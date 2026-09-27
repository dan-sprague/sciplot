//! `contour`: lines of constant value of a 2D field (Makie's `contour`), e.g. level sets and
//! nullclines.
//!
//! The lines come from marching squares ([`super::marching`], Contour.jl's rules) on the grid
//! points: `z[i, j]` sits at `(x_i, y_j)`. Segments are joined into polylines, and lines that
//! close on themselves are drawn as closed loops, so dash patterns and joins run around them.

use super::marching::{self, GridField, Polyline};
use super::{ColorSpec, PlotImpl, PlotKind, add_to_axis, plot_common};
use crate::attrs::{Conv, attributes};
use crate::color::{Color, Colormap};
use crate::data::{CellCoords, CellSpecKind as Spec, Data2D, Scalar};
use crate::figure::{Dirty, FigShared, PlotId};
use crate::scene::drawlist::{Buf, BufKey, GlyphsPrim, LinesPrim, Prim, PrimColor};
use crate::scene::{AxisFrame, PlotCtx};
use crate::style::{JoinStyle, LineCap, Linestyle};
use crate::text::{Font, RichText};
use crate::ticks::TickFormat;
use crate::transform::Scale;
use parking_lot::Mutex;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Contour levels (Makie's `levels`): a number of automatic levels, or explicit values.
///
/// `levels = 5` converts to `Levels::Count(5)`; a vector or array of values (`levels = [0.0]`,
/// `levels = linspace(0.0, 1.0, 11)`) to `Levels::Values`.
#[derive(Clone, Debug, PartialEq)]
pub enum Levels {
    /// `n` levels chosen from the data's range: evenly spaced lines strictly inside it for
    /// `contour`, `n` equal bands covering it for `contourf`.
    Count(usize),
    /// Explicit values: the lines of `contour`, the band edges (ascending) of `contourf`.
    Values(Arc<Vec<f64>>),
}

macro_rules! levels_from_int {
    ($($t:ty),*) => {$(
        impl Conv<Levels> for $t {
            fn conv(self) -> Levels {
                Levels::Count(self.max(0) as usize)
            }
        }
    )*};
}
levels_from_int!(i32, i64, u32, u64, usize, isize);

impl Conv<Levels> for Levels {
    fn conv(self) -> Levels {
        self
    }
}
impl<T: Scalar> Conv<Levels> for Vec<T> {
    fn conv(self) -> Levels {
        Levels::Values(Arc::new(self.into_iter().map(|v| v.to_f64()).collect()))
    }
}
impl<T: Scalar> Conv<Levels> for &Vec<T> {
    fn conv(self) -> Levels {
        Levels::Values(Arc::new(self.iter().map(|v| v.to_f64()).collect()))
    }
}
impl<T: Scalar> Conv<Levels> for &[T] {
    fn conv(self) -> Levels {
        Levels::Values(Arc::new(self.iter().map(|v| v.to_f64()).collect()))
    }
}
impl<T: Scalar, const N: usize> Conv<Levels> for [T; N] {
    fn conv(self) -> Levels {
        Levels::Values(Arc::new(self.iter().map(|v| v.to_f64()).collect()))
    }
}

/// A contour plot handle (Makie's `Contour`).
///
/// Nullclines of a planar system `x' = f(x, y)`, `y' = g(x, y)` are the zero level sets of `f`
/// and `g`: sample both on a grid and draw `levels = [0.0]`.
///
/// ```no_run
/// use sciplot::prelude::*;
/// // FitzHugh–Nagumo: v' = v - v³/3 - w + I,  w' = ε (v + a - b w).
/// let (i_ext, a, b, eps) = (0.5, 0.7, 0.8, 0.08);
/// let (n, m) = (200, 150);
/// let vs = linspace(-2.5, 2.5, n);
/// let ws = linspace(-1.0, 2.0, m);
/// let f: Vec<f64> = (0..n * m).map(|k| { let (v, w) = (vs[k % n], ws[k / n]); v - v.powi(3) / 3.0 - w + i_ext }).collect();
/// let g: Vec<f64> = (0..n * m).map(|k| { let (v, w) = (vs[k % n], ws[k / n]); eps * (v + a - b * w) }).collect();
///
/// let fig = Figure::new();
/// let ax = Axis!(fig.at(1, 1); xlabel = "v", ylabel = "w", title = "FitzHugh–Nagumo nullclines");
/// contour!(ax, &vs, &ws, Field::new(&f, n, m); levels = [0.0], color = RED, linewidth = 2, label = "v' = 0");
/// contour!(ax, &vs, &ws, Field::new(&g, n, m); levels = [0.0], color = BLUE, linewidth = 2, label = "w' = 0");
/// axislegend(&ax);
/// fig.save("nullclines.png").unwrap();
/// ```
#[derive(Clone)]
pub struct Contour {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

/// Contour lines of one level, in data space.
#[derive(Debug)]
pub(crate) struct LevelLines {
    pub level: f64,
    pub lines: Vec<Polyline>,
}

/// Lines per level, cached across frames (and shared with snapshots) by grid generation and
/// levels.
type LineCache = Arc<Mutex<Option<(u64, Arc<Vec<LevelLines>>)>>>;

#[derive(Clone, Debug)]
pub(crate) struct ContourState {
    pub field: GridField,
    pub attrs: ContourAttrs,
    pub cache: LineCache,
}

attributes! {
    Contour(ContourAttrs, ContourResolved, ContourTheme) via with_attrs {
        /// Line color: automatic (default) colors each level through `colormap` over
        /// `colorrange`; a single color colors every level; a list of colors (one per level)
        /// colors each level.
        color: ColorSpec = |_| ColorSpec::Auto, STYLE;
        /// Number of automatic levels (default 5, evenly spaced strictly inside the data's range)
        /// or explicit level values.
        levels: super::Levels = |_| super::Levels::Count(5), STYLE;
        /// Line width in units (Makie default 1.0).
        linewidth: f64 = |_| 1.0, STYLE;
        /// Dash pattern of the lines.
        linestyle: Linestyle = |_| Linestyle::Solid, STYLE;
        /// Shape of open line ends.
        linecap: LineCap = |_| LineCap::Butt, STYLE;
        /// Shape of the corners.
        joinstyle: JoinStyle = |_| JoinStyle::Miter, STYLE;
        /// Miter joints sharper than this angle (radians) are beveled (Makie default π/3).
        miter_limit: f64 = |_| std::f64::consts::FRAC_PI_3, STYLE;
        /// Writes each line's level on it, with a gap cut into the line (Makie's `labels`).
        labels: bool = |_| false, STYLE;
        /// Font of the labels.
        labelfont: Font = |_| Font::Regular, STYLE;
        /// Color of the labels (default: the line's color).
        labelcolor: Option<Color> = |_| None, STYLE;
        /// Label text for a level: automatic (rounded to 2 digits, integers without a decimal
        /// point, like Makie), a format string (`"{:.1f}"`) or a closure `|level| -> String`.
        labelformatter: TickFormat = |_| TickFormat::Automatic, STYLE;
        /// Font size of the labels (Makie default 10).
        labelsize: f64 = |_| 10.0, STYLE;
        /// Colormap for automatic colors (default viridis).
        colormap: Colormap = |_| Colormap::VIRIDIS, STYLE;
        /// `(lo, hi)` mapped to the colormap ends; default: the finite extrema of the data.
        colorrange: Option<[f64; 2]> = |_| None, STYLE;
        /// Opacity multiplier.
        alpha: f64 = |_| 1.0, STYLE;
    }
}

plot_common!(Contour);
super::color_mapped!(Contour);

/// Makie's `isapprox` with default tolerances.
pub(crate) fn approx_eq(a: f64, b: f64) -> bool {
    a == b || (a - b).abs() <= f64::EPSILON.sqrt() * a.abs().max(b.abs())
}

/// Makie's `contour_label_formatter`: rounded to 2 digits; integers print without a decimal point.
pub(crate) fn default_label(level: f64) -> String {
    // Julia's `round(level; digits = 2)` rounds ties to even.
    let r = (level * 100.0).round_ties_even() / 100.0;
    if r.fract() == 0.0 && r.abs() < 1e15 { format!("{}", r as i64) } else { format!("{r}") }
}

fn label_text(fmt: &TickFormat, level: f64) -> RichText {
    match fmt {
        TickFormat::Automatic => default_label(level).into(),
        TickFormat::Func(f) => f(level),
        TickFormat::Format(s) => crate::ticks::format_with(s, level).into(),
    }
}

/// Makie's `to_upright_angle`: maps an angle to `[-π/2, π/2]` so text never reads upside down.
fn upright(a: f64) -> f64 {
    if a.abs() > std::f64::consts::FRAC_PI_2 { a - std::f64::consts::PI.copysign(a) } else { a }
}

impl ContourState {
    /// The levels in effect (Makie's `zlevels`): none when the data is constant.
    pub(crate) fn levels(&self, levels: &super::Levels) -> Vec<f64> {
        let Some((lo, hi)) = self.field.zrange() else { return Vec::new() };
        if approx_eq(lo, hi) {
            return Vec::new();
        }
        match levels {
            super::Levels::Count(n) => {
                let dz = (hi - lo) / (*n as f64 + 1.0);
                (1..=*n).map(|k| lo + dz * k as f64).collect()
            }
            super::Levels::Values(v) => v.iter().copied().filter(|v| v.is_finite()).collect(),
        }
    }

    /// The colorrange in effect (Makie's `computed_colorrange`).
    fn colorrange(&self, r: &ContourResolved) -> (f64, f64) {
        let (lo, hi) = match (r.colorrange, self.field.zrange()) {
            (Some([a, b]), _) => (a, b),
            (None, Some(z)) => z,
            (None, None) => (0.0, 1.0),
        };
        if approx_eq(lo, hi) {
            let d = lo.abs().max(1.0);
            (lo - d, hi + d)
        } else {
            (lo, hi)
        }
    }

    /// One color per level (Makie's `color_per_level`), with `alpha` applied.
    fn level_colors(&self, r: &ContourResolved, levels: &[f64], palette: &[Color], cycle: usize) -> Vec<Color> {
        let a = r.alpha as f32;
        let fade = |c: Color| c.with_alpha(c.a * a);
        let by_level = || {
            let (lo, hi) = self.colorrange(r);
            levels.iter().map(|l| fade(r.colormap.sample((l - lo) / (hi - lo)))).collect()
        };
        match &r.color {
            ColorSpec::PerPoint(cs) if cs.len() == levels.len() => cs.iter().map(|c| fade(*c)).collect(),
            ColorSpec::PerPoint(_) | ColorSpec::Values(_) => {
                crate::warn_once("contour: `color` needs one color per level; coloring levels by the colormap");
                by_level()
            }
            ColorSpec::Auto => by_level(),
            spec => {
                let c = crate::scene::resolve_color(spec, cycle, palette).unwrap_or(Color::rgb(0.0, 0.0, 0.0));
                vec![fade(c); levels.len()]
            }
        }
    }

    /// The contour lines of every level (cached).
    fn lines(&self, levels: &[f64]) -> Arc<Vec<LevelLines>> {
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            self.field.generation.hash(&mut h);
            levels.iter().for_each(|l| l.to_bits().hash(&mut h));
            h.finish()
        };
        let mut cache = self.cache.lock();
        if let Some((k, v)) = cache.as_ref()
            && *k == key
        {
            return v.clone();
        }
        let g = self.field.grid();
        let v: Arc<Vec<LevelLines>> = Arc::new(
            levels
                .iter()
                .map(|&l| LevelLines { level: l, lines: marching::isolines(&g, self.field.level(l)) })
                .collect(),
        );
        *cache = Some((key, v.clone()));
        v
    }
}

/// One drawable piece of a contour line: a polyline of one level in data space.
struct Run {
    level: usize,
    pts: Vec<[f64; 2]>,
    closed: bool,
}

/// A label: level, anchor (figure units), rotation.
struct LabelSpot {
    level: usize,
    anchor: [f64; 2],
    angle: f64,
}

/// The most closed loops drawn as separate closed polylines; more are drawn open.
const MAX_LOOPS: usize = 200;

impl ContourState {
    /// Splits the lines into drawable runs, placing labels and cutting their gaps (Makie's
    /// `label_info` and `masked_lines`).
    fn runs(&self, axis: &AxisFrame, lines: &[LevelLines], r: &ContourResolved) -> (Vec<Run>, Vec<LabelSpot>) {
        let mut runs = Vec::new();
        let mut spots = Vec::new();
        if !r.labels {
            for (li, ll) in lines.iter().enumerate() {
                for p in &ll.lines {
                    runs.push(Run { level: li, pts: p.pts.clone(), closed: p.closed });
                }
            }
            return (runs, spots);
        }
        // Makie skips the gaps when lines are short (fewer than ~10 points each).
        let (npts, nlines) = lines.iter().flat_map(|l| &l.lines).fold((0, 0), |(n, k), p| (n + p.pts.len() + 1, k + 1));
        let mask_gaps = nlines * 10 <= npts;
        let texts: Vec<RichText> = lines.iter().map(|l| label_text(&r.labelformatter, l.level)).collect();
        for (li, ll) in lines.iter().enumerate() {
            for p in &ll.lines {
                let n = p.pts.len();
                if n == 0 {
                    continue;
                }
                let m = n.div_ceil(2) - 1;
                let (p1, p2, p3) = (p.pts[m.saturating_sub(1)], p.pts[m], p.pts[(m + 1).min(n - 1)]);
                let spot = axis.to_units(p2[0], p2[1]).map(|anchor| {
                    let angle = match (axis.to_units(p1[0], p1[1]), axis.to_units(p3[0], p3[1])) {
                        (Some(u1), Some(u3)) => upright((-(u3[1] - u1[1])).atan2(u3[0] - u1[0])),
                        _ => 0.0,
                    };
                    LabelSpot { level: li, anchor, angle }
                });
                let bbox = spot.as_ref().filter(|_| mask_gaps).map(|s| {
                    let l = crate::text::layout(&texts[li], r.labelsize, r.labelfont, Color::TRANSPARENT);
                    crate::text::placed_bbox(&l, s.anchor, (0.5, 0.5), s.angle)
                });
                spots.extend(spot);
                let Some([bx, by, bw, bh]) = bbox else {
                    runs.push(Run { level: li, pts: p.pts.clone(), closed: p.closed });
                    continue;
                };
                let inside = |q: &[f64; 2]| {
                    axis.to_units(q[0], q[1])
                        .is_some_and(|u| u[0] >= bx && u[0] <= bx + bw && u[1] >= by && u[1] <= by + bh)
                };
                let masked: Vec<bool> = p.pts.iter().map(inside).collect();
                // A closed loop's ring without the repeated point, walked from just after a gap.
                let (ring, start) = match (p.closed, masked.iter().position(|m| *m)) {
                    (_, None) => {
                        runs.push(Run { level: li, pts: p.pts.clone(), closed: p.closed });
                        continue;
                    }
                    (true, Some(g)) => (n - 1, g + 1),
                    (false, Some(_)) => (n, 0),
                };
                let mut cur: Vec<[f64; 2]> = Vec::new();
                for k in 0..ring {
                    let idx = (start + k) % ring;
                    if masked[idx] {
                        if cur.len() >= 2 {
                            runs.push(Run { level: li, pts: std::mem::take(&mut cur), closed: false });
                        }
                        cur.clear();
                    } else {
                        cur.push(p.pts[idx]);
                    }
                }
                if cur.len() >= 2 {
                    runs.push(Run { level: li, pts: cur, closed: false });
                }
            }
        }
        (runs, spots)
    }
}

/// Converts data points to the axis' local f32 coordinates.
fn to_local(a: &AxisFrame, pts: &[[f64; 2]], out: &mut Vec<[f32; 2]>) {
    let (xs, ys) = (a.attrs.xscale, a.attrs.yscale);
    out.extend(pts.iter().map(|p| {
        let (sx, sy) = (xs.forward(p[0]), ys.forward(p[1]));
        if sx.is_finite() && sy.is_finite() { a.rebase.to_local(sx, sy) } else { [f32::NAN; 2] }
    }));
}

/// Draw-ready line geometry: one NaN-separated buffer of open runs (with their levels), one
/// buffer per closed loop, and the labels.
struct LineGeom {
    open: Arc<Vec<[f32; 2]>>,
    open_level: Vec<u32>,
    loops: Vec<(usize, Arc<Vec<[f32; 2]>>)>,
    spots: Vec<LabelSpot>,
}

impl PlotImpl for ContourState {
    fn cycle_group(&self) -> &'static str {
        "contour"
    }

    fn color_is_auto(&self, _theme: &crate::theme::Theme) -> bool {
        false
    }

    fn data_bounds(&self, xs: Scale, ys: Scale) -> Option<[f64; 4]> {
        self.field.bounds(xs, ys)
    }

    fn legend_elements(&self, ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        // Makie's contour legend shows its lines' color when it is one color, else the default.
        let r = self.attrs.resolve(&ctx.theme.contour, ctx.g);
        let color = match &r.color {
            ColorSpec::Solid(_) | ColorSpec::Cycled(_) => {
                ctx.color(&r.color, false, r.alpha, super::legend_elements::DEFAULT_LINECOLOR)
            }
            _ => super::legend_elements::DEFAULT_LINECOLOR,
        };
        vec![super::LegendElement::Line { color, linewidth: r.linewidth, linestyle: r.linestyle }]
    }

    fn colormapping(&self, theme: &crate::theme::Theme, g: &crate::theme::Globals) -> Option<super::ResolvedColormap> {
        let r = self.attrs.resolve(&theme.contour, g);
        let (lo, hi) = self.colorrange(&r);
        Some(super::ResolvedColormap {
            colormap: r.colormap,
            colorrange: (lo, hi),
            lowclip: None,
            highclip: None,
            alpha: 1.0,
            mapped: matches!(r.color, ColorSpec::Auto),
        })
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.contour, ctx.g);
        let levels = self.levels(&r.levels);
        if levels.is_empty() {
            return;
        }
        let colors = self.level_colors(&r, &levels, &ctx.g.palette, ctx.cycle);
        let lines = self.lines(&levels);

        // Geometry key: the lines, the local conversion and (with labels) the screen transform.
        let geom_key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.field.generation, ctx.conv_key(0), r.labels).hash(&mut h);
            levels.iter().for_each(|l| l.to_bits().hash(&mut h));
            if r.labels {
                let a = ctx.axis;
                let rect = [a.rect.x, a.rect.y, a.rect.w, a.rect.h];
                (a.view.map(f64::to_bits), rect.map(f64::to_bits), a.attrs.xreversed, a.attrs.yreversed).hash(&mut h);
                (r.labelsize.to_bits(), r.labelfont as u8).hash(&mut h);
                for l in &levels {
                    label_text(&r.labelformatter, *l).plain_text().hash(&mut h);
                }
            }
            h.finish()
        };
        let axis = ctx.axis;
        let build = || {
            let (runs, spots) = self.runs(axis, &lines, &r);
            let mut open = Vec::new();
            let mut open_level = Vec::new();
            let mut loops = Vec::new();
            for run in &runs {
                if run.closed && loops.len() < MAX_LOOPS {
                    let mut v = Vec::with_capacity(run.pts.len());
                    to_local(axis, &run.pts, &mut v);
                    loops.push((run.level, Arc::new(v)));
                } else {
                    if !open.is_empty() {
                        open.push([f32::NAN; 2]);
                        open_level.push(run.level as u32);
                    }
                    to_local(axis, &run.pts, &mut open);
                    open_level.resize(open.len(), run.level as u32);
                }
            }
            vec![LineGeom { open: Arc::new(open), open_level, loops, spots }]
        };
        let geom = ctx.cache.memo(ctx.uid, 255, geom_key, build);
        let Some(geom) = geom.first() else { return };

        let style = |pts: Buf<[f32; 2]>, color: PrimColor, closed: bool| {
            Prim::Lines(LinesPrim {
                pts,
                color,
                width: r.linewidth as f32,
                pattern: r.linestyle.pattern(),
                cap: r.linecap,
                join: r.joinstyle,
                miter_limit: r.miter_limit as f32,
                segments: false,
                closed,
                append: false,
            })
        };
        if geom.open.len() >= 2 {
            let uniform = colors.windows(2).all(|w| w[0] == w[1]);
            let color = if uniform {
                PrimColor::Uniform(colors[0])
            } else {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                geom_key.hash(&mut h);
                colors.iter().for_each(|c| c.to_premul_u32().hash(&mut h));
                let key = h.finish();
                let data = geom.open_level.iter().map(|l| colors[*l as usize].to_premul_u32()).collect();
                PrimColor::PerElement(Buf {
                    key: Some(BufKey { uid: ctx.uid, part: 1, rev: key }),
                    data: Arc::new(data),
                })
            };
            let pts = ctx.keyed_buf(0, geom_key, geom.open.clone());
            ctx.push_data(style(pts, color, false));
        }
        for (k, (level, pts)) in geom.loops.iter().enumerate() {
            let pts = ctx.keyed_buf(2 + k as u8, geom_key, pts.clone());
            ctx.push_data(style(pts, PrimColor::Uniform(colors[*level]), true));
        }

        if !geom.spots.is_empty() {
            let mut glyphs = Vec::new();
            for s in &geom.spots {
                let color = r.labelcolor.unwrap_or(colors[s.level]);
                let text = label_text(&r.labelformatter, levels[s.level]);
                let l = crate::text::layout(&text, r.labelsize, r.labelfont, color);
                glyphs.extend(crate::text::place(&l, s.anchor, (0.5, 0.5), s.angle));
            }
            ctx.push_figure(Prim::Glyphs(GlyphsPrim { glyphs }));
        }
    }

    fn pick(&self, ctx: &mut super::pick::PickCtx<'_>) -> Option<super::pick::Hover> {
        let r = self.attrs.resolve(&ctx.theme.contour, ctx.g);
        let levels = self.levels(&r.levels);
        let lines = self.lines(&levels);
        let mut best: Option<(f64, [f64; 2], [f64; 2], f64)> = None;
        for ll in lines.iter() {
            for p in &ll.lines {
                if let Some((d, u, q)) = ctx.nearest_on_polyline(p.pts.iter())
                    && best.is_none_or(|b| d < b.0)
                {
                    best = Some((d, u, q, ll.level));
                }
            }
        }
        let (dist, anchor, [x, y], level) = best?;
        let text = format!("{}\nlevel: {}", super::pick::point_text(x, y), super::pick::sig6(level));
        Some(super::pick::Hover { dist, anchor, text, ring: None, outline: None })
    }
}

#[track_caller]
fn create_or_panic(what: &str, x: Spec, y: Spec, z: &impl Data2D) -> GridField {
    GridField::new(x, y, z).unwrap_or_else(|e| panic!("{what} {e}"))
}

impl Contour {
    fn with_attrs(&self, f: impl FnOnce(&mut ContourAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Contour(s) = &mut p.kind {
                f(&mut s.attrs)
            }
        });
    }

    #[track_caller]
    pub(crate) fn create(ax: &crate::Axis, x: Spec, y: Spec, z: impl Data2D) -> Contour {
        let field = create_or_panic("contour", x, y, &z);
        let st = ContourState { field, attrs: ContourAttrs::default(), cache: LineCache::default() };
        Contour { sh: ax.sh.clone(), id: add_to_axis(ax, PlotKind::Contour(st)) }
    }

    /// Replaces the values (converted on the calling thread). New dimensions recompute the grid
    /// coordinates from the ones the plot was created with (vector coordinates must then match).
    ///
    /// # Panics
    /// If the dimensions changed and explicit coordinate vectors no longer fit.
    #[track_caller]
    pub fn set_data(&self, z: impl Data2D) -> Contour {
        let dims = z.dims();
        let (values, enc) = marching::encode(&z);
        let res = self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            p.data_rev += 1;
            match &mut p.kind {
                PlotKind::Contour(s) => s.field.set_values(dims, values, enc),
                _ => Ok(()),
            }
        });
        if let Some(Err(e)) = res {
            panic!("Contour::set_data {e}");
        }
        self.clone()
    }

    /// Replaces the grid coordinates (same rules as [`Axis::contour_xy`](crate::Axis::contour_xy)).
    ///
    /// # Panics
    /// If a coordinate vector's length differs from the number of grid points.
    #[track_caller]
    pub fn set_coords(&self, x: impl CellCoords, y: impl CellCoords) -> Contour {
        let (xs, ys) = (x.cell_spec().0, y.cell_spec().0);
        let res = self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            p.data_rev += 1;
            match &mut p.kind {
                PlotKind::Contour(s) => s.field.set_coords(xs, ys),
                _ => Ok(()),
            }
        });
        if let Some(Err(e)) = res {
            panic!("Contour::set_coords {e}");
        }
        self.clone()
    }

    /// The levels drawn right now (automatic levels computed from the data).
    pub fn resolved_levels(&self) -> Vec<f64> {
        let st = self.sh.state.lock();
        let g = st.theme.globals();
        match st.plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Contour(s)) => s.levels(&s.attrs.resolve(&st.theme.contour, &g).levels),
            _ => Vec::new(),
        }
    }
}

impl crate::Axis {
    /// Makie's `contour!(ax, z)`: contour lines of `z`, whose point `(i, j)` (0-based) sits at
    /// `(i + 1, j + 1)`.
    #[track_caller]
    pub fn contour(&self, z: impl Data2D) -> Contour {
        Contour::create(self, Spec::Index, Spec::Index, z)
    }

    /// Makie's `contour!(ax, x, y, z)`: `z[i, j]` is the value at the grid point `(x_i, y_j)`.
    /// `x` and `y` are [`CellCoords`], read as point positions: `a..=b` or
    /// [`Edges(a, b)`](crate::Edges) spread the points evenly from `a` to `b`, and a vector gives
    /// each point (it must have one value per point).
    ///
    /// ```no_run
    /// use sciplot::prelude::*;
    /// let (nx, ny) = (100, 80);
    /// let xs = linspace(-2.0, 2.0, nx);
    /// let ys = linspace(-1.5, 1.5, ny);
    /// let z: Vec<f64> = (0..nx * ny).map(|k| xs[k % nx].powi(2) + 2.0 * ys[k / nx].powi(2)).collect();
    /// let fig = Figure::new();
    /// let ax = Axis::new(fig.at(1, 1));
    /// contour!(ax, &xs, &ys, Field::new(&z, nx, ny); levels = 8, labels = true);
    /// ```
    #[track_caller]
    pub fn contour_xy(&self, x: impl CellCoords, y: impl CellCoords, z: impl Data2D) -> Contour {
        Contour::create(self, x.cell_spec().0, y.cell_spec().0, z)
    }
}

impl crate::GridPosition {
    /// Makie's `contour(fig[r, c], z)`: a new Axis at this position with a contour plot.
    #[track_caller]
    pub fn contour(&self, z: impl Data2D) -> Contour {
        crate::Axis::new(self.clone()).contour(z)
    }

    /// Makie's `contour(fig[r, c], x, y, z)`: a new Axis at this position with a contour plot.
    #[track_caller]
    pub fn contour_xy(&self, x: impl CellCoords, y: impl CellCoords, z: impl Data2D) -> Contour {
        crate::Axis::new(self.clone()).contour_xy(x, y, z)
    }
}

/// Makie's `contour(z)`: a new Figure and Axis with contour lines of `z` (points at `1..=nx`,
/// `1..=ny`). Returns the plot handle; call `.save(..)` or `.show()` on it.
///
/// ```no_run
/// let (nx, ny) = (60, 40);
/// let z: Vec<f64> = (0..nx * ny).map(|k| ((k % nx) as f64 * 0.1).sin() * ((k / nx) as f64 * 0.15).cos()).collect();
/// sciplot::contour(sciplot::Field::new(&z, nx, ny)).save("contour.png").unwrap();
/// ```
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn contour(z: impl Data2D) -> Contour {
    crate::Figure::new().at(1, 1).contour(z)
}

/// Makie's `contour(x, y, z)`: a new Figure and Axis with contour lines on the given grid points
/// (see [`Axis::contour_xy`](crate::Axis::contour_xy)).
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn contour_xy(x: impl CellCoords, y: impl CellCoords, z: impl Data2D) -> Contour {
    crate::Figure::new().at(1, 1).contour_xy(x, y, z)
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::prelude::*;
    use crate::scene::drawlist::{DrawList, LinesPrim, Prim, PrimColor, Space};
    use crate::scene::{AxisFrame, SceneCache};

    /// The Gaussian mixture of `examples/contour_check.rs` / `tools/contour_check.jl`.
    pub(crate) fn mixture() -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let g = |x: f64, y: f64, x0: f64, y0: f64, s: f64, a: f64| {
            a * (-((x - x0).powi(2) + (y - y0).powi(2)) / (2.0 * s * s)).exp()
        };
        let f = |x, y| g(x, y, -1.0, -0.5, 0.6, 1.0) + g(x, y, 1.2, 0.8, 0.8, 0.8) + g(x, y, 0.8, -1.2, 0.4, 0.6);
        let (nx, ny) = (120, 100);
        let xs = linspace(-3.0, 3.0, nx);
        let ys = linspace(-2.5, 2.5, ny);
        let z = (0..nx * ny).map(|k| f(xs[k % nx], ys[k / nx])).collect();
        (xs, ys, z)
    }

    pub(crate) fn build(fig: &Figure) -> (DrawList, Vec<AxisFrame>) {
        crate::scene::build(&fig.sh.snapshot(), None, &mut SceneCache::new())
    }

    fn lines(dl: &DrawList) -> Vec<&LinesPrim> {
        dl.items
            .iter()
            .filter_map(|i| match &i.prim {
                Prim::Lines(l) if i.space != Space::Figure => Some(l),
                _ => None,
            })
            .collect()
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Confirmed with CairoMakie (tools/contour_check.jl): `levels = 8` on the mixture and the
    /// axis limits (5 % margins, not tight).
    #[test]
    fn automatic_levels_and_limits_match_makie() {
        let (xs, ys, z) = mixture();
        let c = contour_xy(&xs, &ys, Field::new(&z, 120, 100)).levels(8);
        let makie = [0.11160228, 0.22320445, 0.33480662, 0.4464088, 0.55801094, 0.6696131, 0.7812153, 0.8928175];
        let lv = c.resolved_levels();
        assert_eq!(lv.len(), 8);
        for (a, b) in lv.iter().zip(makie) {
            assert!(close(*a, b, 1e-6), "{lv:?}");
        }
        let lim = build(&c.figure()).1[0].limits;
        for (a, b) in lim.iter().zip([-3.3, 3.3, -2.75, 2.75]) {
            assert!(close(*a, b, 1e-6), "{lim:?}");
        }
        // Constant data has no levels (Makie's `isapprox(zmin, zmax)`).
        assert!(contour(Field::new(&[1.0; 4], 2, 2)).levels([1.0]).resolved_levels().is_empty());
    }

    /// Makie's `contour_label_formatter` outputs (printed by CairoMakie).
    #[test]
    fn label_formatter_matches_makie() {
        let f = super::default_label;
        let cases = [
            (0.125, "0.12"),
            (0.135, "0.14"),
            (0.111602, "0.11"),
            (1.0, "1"),
            (-0.5, "-0.5"),
            (2.0000001, "2"),
            (1234.5678, "1234.57"),
            (1e-5, "0"),
            (-0.004, "0"),
        ];
        for (v, s) in cases {
            assert_eq!(f(v), s, "{v}");
        }
    }

    /// Circles around a minimum are closed loops (joined, not capped); labels cut a gap into
    /// each loop, which then becomes an open line, and add one label per loop.
    #[test]
    fn closed_loops_and_label_gaps() {
        let n = 41;
        let xs = linspace(-1.0, 1.0, n);
        let z: Vec<f64> = (0..n * n).map(|k| xs[k % n].powi(2) + xs[k / n].powi(2)).collect();
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        let c = ax.contour_xy(&xs, &xs, Field::new(&z, n, n)).levels([0.25, 0.5]);
        let (dl, _) = build(&fig);
        let l = lines(&dl);
        assert_eq!(l.len(), 2, "one closed polyline per level");
        assert!(l.iter().all(|l| l.closed && l.pts.data.first() == l.pts.data.last()));

        c.labels(true);
        let (dl, _) = build(&fig);
        let l = lines(&dl);
        assert_eq!(l.len(), 1, "labelled loops are open lines in one buffer");
        assert!(!l[0].closed);
        let breaks = l[0].pts.data.iter().filter(|p| p[0].is_nan()).count();
        assert_eq!(breaks, 1, "two open runs");
        let glyphs: usize = dl
            .items
            .iter()
            .map(|i| match &i.prim {
                Prim::Glyphs(g) if i.z == 0.0 => g.glyphs.len(),
                _ => 0,
            })
            .sum();
        assert_eq!(glyphs, "0.25".len() + "0.5".len());
    }

    /// Nullclines: the zero level of `y - x²` lies on the parabola.
    #[test]
    fn nullcline_follows_the_zero_set() {
        let (nx, ny) = (81, 61);
        let xs = linspace(-2.0, 2.0, nx);
        let ys = linspace(-1.0, 3.0, ny);
        let z: Vec<f64> = (0..nx * ny).map(|k| ys[k / nx] - xs[k % nx].powi(2)).collect();
        let c = contour_xy(&xs, &ys, Field::new(&z, nx, ny)).levels([0.0]).color(RED);
        let (dl, axes) = build(&c.figure());
        let l = lines(&dl);
        assert_eq!(l.len(), 1);
        assert!(matches!(l[0].color, PrimColor::Uniform(c) if c == RED));
        let r = axes[0].rebase;
        let mut n = 0;
        for p in l[0].pts.data.iter().filter(|p| p[0].is_finite()) {
            let (x, y) = (p[0] as f64 / r.k[0] + r.origin[0], p[1] as f64 / r.k[1] + r.origin[1]);
            // Linear interpolation of a parabola sampled every 0.05 in x.
            assert!((y - x * x).abs() < 0.01, "({x}, {y})");
            n += 1;
        }
        assert!(n > 100, "{n} points");
    }

    /// Automatic colors map each level through the colormap over the data range; a list of
    /// colors gives one per level; the colorbar sees the level mapping.
    #[test]
    fn level_colors() {
        let z: Vec<f64> = (0..16).map(|k| k as f64).collect();
        let c = contour(Field::new(&z, 4, 4)).levels([3.75, 7.5, 11.25]);
        let m = c.colormapping().unwrap();
        assert_eq!((m.colorrange, m.mapped), ((0.0, 15.0), true));
        let st = c.sh.snapshot();
        let Some(crate::plots::PlotKind::Contour(s)) = st.plot(c.id).map(|p| &p.kind) else { unreachable!() };
        let r = s.attrs.resolve(&st.theme.contour, &st.theme.globals());
        let cs = s.level_colors(&r, &[3.75, 7.5, 11.25], &[], 0);
        assert_eq!(cs[1], Colormap::VIRIDIS.sample(0.5));
        let (dl, _) = build(&c.figure());
        assert!(lines(&dl).iter().all(|l| matches!(l.color, PrimColor::PerElement(_))));
        c.color(vec![RED, GREEN, BLUE]).alpha(0.5);
        let st = c.sh.snapshot();
        let Some(crate::plots::PlotKind::Contour(s)) = st.plot(c.id).map(|p| &p.kind) else { unreachable!() };
        let r = s.attrs.resolve(&st.theme.contour, &st.theme.globals());
        assert_eq!(s.level_colors(&r, &[3.75, 7.5, 11.25], &[], 0)[2], BLUE.with_alpha(0.5));
        assert!(!c.colormapping().unwrap().mapped);
    }

    #[test]
    fn set_data_updates_lines_and_levels() {
        let c = contour(Field::new(&[0.0, 1.0, 2.0, 3.0], 2, 2));
        assert_eq!(c.resolved_levels().len(), 5);
        c.set_data(Field::new(&[0.0, 10.0, 20.0, 30.0, 40.0, 50.0], 3, 2)).levels(2);
        let lv = c.resolved_levels();
        assert!((lv[0] - 50.0 / 3.0).abs() < 1e-9 && (lv[1] - 100.0 / 3.0).abs() < 1e-9, "{lv:?}");
        let (dl, _) = build(&c.figure());
        assert_eq!(lines(&dl).len(), 1);
        let bad = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.set_coords([1.0, 2.0], [0.0, 1.0])));
        assert!(bad.is_err());
    }

    #[test]
    fn pick_reports_the_level() {
        let n = 21;
        let xs = linspace(-1.0, 1.0, n);
        let z: Vec<f64> = (0..n * n).map(|k| xs[k % n]).collect();
        let c = contour_xy(&xs, &xs, Field::new(&z, n, n)).levels([0.5]);
        let st = c.sh.snapshot();
        let (_, axes) = crate::scene::build(&st, None, &mut SceneCache::new());
        let a = &axes[0];
        let g = st.theme.globals();
        let p = st.plots[0].as_ref().unwrap();
        let mut cache = crate::plots::pick::PickCache::default();
        let on = a.to_units(0.5, 0.0).unwrap();
        let mut ctx = crate::plots::pick::PickCtx {
            axis: a,
            cursor: [on[0] + 3.0, on[1]],
            radius: 10.0,
            uid: p.uid,
            data_rev: p.data_rev,
            theme: &st.theme,
            g: &g,
            cache: &mut cache,
        };
        let h = p.kind.imp().pick(&mut ctx).expect("near the line");
        assert!((h.dist - 3.0).abs() < 1e-6, "{}", h.dist);
        assert!(h.text.ends_with("level: 0.5"), "{}", h.text);
    }
}
