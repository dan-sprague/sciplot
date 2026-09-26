//! `arrows` (Makie's `arrows2d`, a.k.a. quiver): arrows from positions along direction vectors.
//!
//! Positions and directions live in data space, while the arrow shape (tail, shaft and tip widths
//! and lengths) is in screen units, like Makie's default `markerspace = :pixel`. Each frame the
//! start and end points are projected through the axis and every arrow's triangles are built on
//! the CPU in figure units, so pan/zoom keeps widths constant while lengths follow the data. The
//! mesh is memoized per axis geometry, so frames that do not move the axis reuse it.

use super::{ColorSpec, PlotImpl, PlotKind, add_to_axis, plot_common, point_bounds, zip_xy};
use crate::attrs::{Conv, attributes, conv_identity};
use crate::color::{Color, Colormap, MappingAttrs, ValueEncoding};
use crate::data::{Data1D, Num, PointData};
use crate::figure::{Dirty, FigShared, PlotId};
use crate::scene::drawlist::{MeshPrim, MeshVertex, Prim};
use crate::scene::{AxisFrame, PlotCtx};
use crate::style::{Linestyle, Marker};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// An arrows plot handle (Makie's `Arrows2D`, also known as quiver).
///
/// ```no_run
/// use ezviz::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// // Damped pendulum phase portrait on a 20 × 20 grid, colored by speed.
/// let g = linspace(-3.0, 3.0, 20);
/// let ar = ax.arrows_fn(&g, &g, |x, y| (y, -x.sin() - 0.2 * y));
/// ar.color(Magnitude).normalize(true).lengthscale(0.25).align(ArrowAlign::Center);
/// Colorbar::new(fig.at(1, 2), &ar);
/// fig.save("quiver.png").unwrap();
/// ```
#[doc(alias = "quiver")]
#[doc(alias = "arrows2d")]
#[derive(Clone)]
pub struct Arrows {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct ArrowsState {
    /// Anchor positions (data space).
    pub pos: Arc<Vec<[f64; 2]>>,
    /// Direction vectors (data space), one per position.
    pub dir: Arc<Vec<[f64; 2]>>,
    pub attrs: ArrowsAttrs,
    /// Bumped by every attribute change (keys the memoized mesh).
    pub style_rev: u64,
}

/// Which part of an arrow sits at its position (Makie's `align`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArrowAlign {
    /// The tail is at the position: the arrow points away from it (Makie's default `:tail`).
    Tail,
    /// The middle of the arrow is at the position (`:center`).
    Center,
    /// The tip is at the position: the arrow points at it (`:tip`).
    Tip,
    /// This fraction of the arrow (0 = tail, 1 = tip) is at the position; values outside 0..=1
    /// leave a gap between the arrow and its position.
    Fraction(f64),
}

impl ArrowAlign {
    /// The fraction of the arrow placed at the position (Makie's `_arrow_align_val`).
    pub fn fraction(self) -> f64 {
        match self {
            ArrowAlign::Tail => 0.0,
            ArrowAlign::Center => 0.5,
            ArrowAlign::Tip => 1.0,
            ArrowAlign::Fraction(f) => f,
        }
    }
}

conv_identity!(ArrowAlign);
impl<N: Num> Conv<ArrowAlign> for N {
    fn conv(self) -> ArrowAlign {
        ArrowAlign::Fraction(self.to_f64())
    }
}

/// Colors each vector by its length through the colormap: `arrows(..).color(Magnitude)`
/// (`streamplot`'s default, Makie's `color = norm`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Magnitude;

/// A scalar function of a vector `(u, v)`, mapped through a colormap.
pub type ScalarFn = Arc<dyn Fn(f64, f64) -> f64 + Send + Sync>;

/// The color of a vector-field plot (`arrows`, `streamplot`): a [`ColorSpec`] (one color,
/// per-arrow colors or values), the vector magnitude, or any scalar function of the vector, both
/// mapped through the plot's colormap.
#[derive(Clone)]
pub enum VectorColor {
    /// A color, `Cycled(i)`, per-arrow colors or per-arrow values.
    Spec(ColorSpec),
    /// The vector's length `hypot(u, v)`, mapped through the colormap.
    Magnitude,
    /// `f(u, v)` of each vector, mapped through the colormap (see `color_fn`).
    Func(ScalarFn),
}

impl std::fmt::Debug for VectorColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VectorColor::Spec(s) => f.debug_tuple("Spec").field(s).finish(),
            VectorColor::Magnitude => f.write_str("Magnitude"),
            VectorColor::Func(g) => write!(f, "Func({:p})", Arc::as_ptr(g)),
        }
    }
}

impl VectorColor {
    /// The scalar a vector maps to (`None` for plain colors).
    pub(crate) fn scalar(&self, u: f64, v: f64) -> Option<f64> {
        match self {
            VectorColor::Magnitude => Some((u * u + v * v).sqrt()),
            VectorColor::Func(f) => Some(f(u, v)),
            VectorColor::Spec(_) => None,
        }
    }

    /// Identity of the scalar function (0 for the built-in choices), for memo keys.
    pub(crate) fn func_id(&self) -> usize {
        match self {
            VectorColor::Func(f) => Arc::as_ptr(f) as *const () as usize,
            VectorColor::Magnitude => 1,
            VectorColor::Spec(_) => 0,
        }
    }
}

impl<T: Conv<ColorSpec>> Conv<VectorColor> for T {
    #[track_caller]
    fn conv(self) -> VectorColor {
        VectorColor::Spec(self.conv())
    }
}
impl Conv<VectorColor> for Magnitude {
    fn conv(self) -> VectorColor {
        VectorColor::Magnitude
    }
}
impl Conv<VectorColor> for VectorColor {
    fn conv(self) -> VectorColor {
        self
    }
}

attributes! {
    Arrows(ArrowsAttrs, ArrowsResolved, ArrowsTheme) via with_attrs {
        /// A color (Makie default black; arrows do not cycle), `Cycled(i)`, per-arrow colors,
        /// per-arrow values, or [`Magnitude`] (values mapped through the colormap).
        color: VectorColor = |_| VectorColor::Spec(ColorSpec::Solid(Color::rgb(0.0, 0.0, 0.0))), STYLE;
        /// Colormap for mapped colors (default viridis).
        colormap: Colormap = |_| Colormap::VIRIDIS, STYLE;
        /// `(lo, hi)` mapped to the colormap ends; default: the finite extrema of the values.
        colorrange: Option<[f64; 2]> = |_| None, STYLE;
        /// Color for values below the colorrange (default: the first colormap color).
        lowclip: Option<Color> = |_| None, STYLE;
        /// Color for values above the colorrange (default: the last colormap color).
        highclip: Option<Color> = |_| None, STYLE;
        /// Color for NaN values (default transparent).
        nan_color: Color = |_| Color::TRANSPARENT, STYLE;
        /// Opacity multiplier.
        alpha: f64 = |_| 1.0, STYLE;
        /// Which part of the arrow sits at the position (default the tail).
        align: ArrowAlign = |_| ArrowAlign::Tail, LIMITS;
        /// Scales every direction vector (data space; default 1).
        lengthscale: f64 = |_| 1.0, LIMITS;
        /// Scale every direction to unit length (before `lengthscale`).
        normalize: bool = |_| false, LIMITS;
        /// Tail width in units (drawn only when `taillength > 0`; default 14).
        tailwidth: f64 = |_| 14.0, STYLE;
        /// Tail length in units (default 0: no tail).
        taillength: f64 = |_| 0.0, STYLE;
        /// Shaft width in units (default 3).
        shaftwidth: f64 = |_| 3.0, STYLE;
        /// Fixed shaft length in units; `None` (default) derives it from the arrow's length. A
        /// fixed length scales the whole arrow to fit.
        shaftlength: Option<f64> = |_| None, STYLE;
        /// Shortest automatic shaft in units; shorter arrows shrink as a whole (default 10).
        minshaftlength: f64 = |_| 10.0, STYLE;
        /// Longest automatic shaft in units (default unbounded).
        maxshaftlength: f64 = |_| f64::INFINITY, STYLE;
        /// Tip width in units (default 14).
        tipwidth: f64 = |_| 14.0, STYLE;
        /// Tip length in units (default 8; 0 draws no tip).
        tiplength: f64 = |_| 8.0, STYLE;
        /// Makie's `strokemask`: every part is drawn this much narrower plus an outline this wide
        /// in its color, i.e. grown by half of it on every side with mitered corners (default 0.75).
        strokemask: f64 = |_| 0.75, STYLE;
    }
}

plot_common!(Arrows);
super::color_mapped!(Arrows);

/// A colormap applied on the CPU, with Makie's clipping rules (like the GPU's `cmap_lookup`).
pub(crate) struct CpuColormap {
    lut: Arc<Vec<Color>>,
    range: [f64; 2],
    lowclip: Color,
    highclip: Color,
    nan_color: Color,
    alpha: f32,
}

impl CpuColormap {
    /// The mapping for `m`, with the automatic colorrange taken from the finite extrema of `values`.
    pub(crate) fn new(m: &MappingAttrs<'_>, values: impl IntoIterator<Item = f64>) -> CpuColormap {
        let range =
            m.colorrange.unwrap_or_else(|| ValueEncoding::new(crate::data::finite_extrema(values)).auto_range());
        CpuColormap {
            lut: m.colormap.lut(),
            range,
            lowclip: m.lowclip.unwrap_or_else(|| m.colormap.first()),
            highclip: m.highclip.unwrap_or_else(|| m.colormap.last()),
            nan_color: m.nan_color,
            alpha: m.alpha as f32,
        }
    }

    /// The straight-alpha color of `v`.
    pub(crate) fn color(&self, v: f64) -> Color {
        let [lo, hi] = self.range;
        let c = if v.is_nan() {
            self.nan_color
        } else if v < lo {
            self.lowclip
        } else if v > hi {
            self.highclip
        } else {
            let t = if hi > lo { (v - lo) / (hi - lo) } else { 0.5 };
            let n = self.lut.len();
            if n == 0 {
                return Color::TRANSPARENT;
            }
            let f = (t.clamp(0.0, 1.0) * (n - 1) as f64) as f32;
            let i = (f.floor() as usize).min(n - 1);
            self.lut[i].lerp(self.lut[(i + 1).min(n - 1)], f - i as f32)
        };
        c.with_alpha(c.a * self.alpha)
    }
}

/// A hash of everything that places an axis' data on screen (rect, limits, scales, reversal).
pub(crate) fn axis_geometry_key(a: &AxisFrame) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let r = a.rect;
    for v in [r.x, r.y, r.w, r.h].into_iter().chain(a.view) {
        v.to_bits().hash(&mut h);
    }
    (a.attrs.xscale, a.attrs.yscale, a.attrs.xreversed, a.attrs.yreversed).hash(&mut h);
    h.finish()
}

/// Makie's `arrow_metrics` for an arrow `len` units long:
/// `[taillength, tailwidth, shaftlength, shaftwidth, tiplength, tipwidth]`, all scaled so the
/// parts add up to `len`.
pub(crate) fn arrow_metrics(len: f64, r: &ArrowsResolved) -> [f64; 6] {
    let shaft = match r.shaftlength {
        Some(s) => s,
        None => (len - r.taillength - r.tiplength).max(r.minshaftlength).min(r.maxshaftlength),
    };
    let total = shaft + r.taillength + r.tiplength;
    let s = if total != 0.0 { len / total } else { 0.0 };
    [r.taillength, r.tailwidth, shaft, r.shaftwidth, r.tiplength, r.tipwidth].map(|v| v * s)
}

/// Offsets a simple polygon outward by `d` with mitered corners (miter length capped at 10 `d`,
/// Cairo's default miter limit). Reproduces the outline of Makie's `strokemask`.
fn offset_polygon<const N: usize>(p: [[f64; 2]; N], d: f64) -> [[f64; 2]; N] {
    let area: f64 = (0..N).map(|i| p[i][0] * p[(i + 1) % N][1] - p[(i + 1) % N][0] * p[i][1]).sum();
    let s = if area < 0.0 { -1.0 } else { 1.0 };
    let normals: [[f64; 2]; N] = std::array::from_fn(|i| {
        let (a, b) = (p[i], p[(i + 1) % N]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = dx.hypot(dy);
        if l > 0.0 { [s * dy / l, -s * dx / l] } else { [0.0, 0.0] }
    });
    std::array::from_fn(|i| {
        let (n1, n2) = (normals[(i + N - 1) % N], normals[i]);
        let k = 1.0 + n1[0] * n2[0] + n1[1] * n2[1];
        let mut m = if k > 1e-9 { [(n1[0] + n2[0]) / k, (n1[1] + n2[1]) / k] } else { n2 };
        let ml = m[0].hypot(m[1]);
        if ml > 10.0 {
            m = [m[0] * 10.0 / ml, m[1] * 10.0 / ml];
        }
        [p[i][0] + d * m[0], p[i][1] + d * m[1]]
    })
}

/// Appends the triangle fan of a star-shaped polygon (from its first vertex), grown by `d`.
fn fan<const N: usize>(out: &mut Vec<MeshVertex>, p: [[f64; 2]; N], d: f64, at: impl Fn([f64; 2]) -> MeshVertex) {
    let v = if d > 0.0 { offset_polygon(p, d) } else { p }.map(at);
    for i in 1..N - 1 {
        out.extend([v[0], v[i], v[i + 1]]);
    }
}

/// Appends the triangles of one arrow from `p0` to `p1` (figure units) with metrics `m`;
/// `parts` says which of tail, shaft and tip are drawn (Makie's `should_component_render`).
///
/// Like Makie, each part's width shrinks by `mask` and the part gets a `mask`-wide outline in the
/// same color, so the drawn part is the shrunk one grown by `mask / 2` with mitered corners.
pub(crate) fn arrow_triangles(
    out: &mut Vec<MeshVertex>,
    p0: [f64; 2],
    p1: [f64; 2],
    m: [f64; 6],
    parts: [bool; 3],
    mask: f64,
    color: u32,
) {
    let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
    let len = dx.hypot(dy);
    if !(len > 0.0 && len.is_finite()) {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    // Local (along, across) -> figure units.
    let at = |[a, b]: [f64; 2]| MeshVertex {
        pos: [(p0[0] + a * ux - b * uy) as f32, (p0[1] + a * uy + b * ux) as f32],
        color,
    };
    let mask = mask.max(0.0);
    let d = 0.5 * mask;
    let mut offset = 0.0;
    for (k, &draw) in parts.iter().enumerate() {
        if !draw {
            continue;
        }
        let (o, l, w) = (offset, m[2 * k], (m[2 * k + 1] - mask).max(0.0));
        offset += l;
        let at_o = |[a, b]: [f64; 2]| at([o + a, b]);
        match k {
            // Makie's `arrowtail2d(l, W, metrics)`: a notched tail, star-shaped from the notch.
            0 if w > 0.0 => {
                let sw = m[3];
                let tail = [
                    [0.0, 0.0],
                    [-0.3 * w, -0.5 * w],
                    [l - 0.3 * w, -0.5 * w],
                    [l, -0.5 * sw],
                    [l, 0.5 * sw],
                    [l - 0.3 * w, 0.5 * w],
                    [-0.3 * w, 0.5 * w],
                ];
                fan(out, tail, d, at_o);
            }
            2 if w > 0.0 => fan(out, [[0.0, -0.5 * w], [l, 0.0], [0.0, 0.5 * w]], d, at_o),
            // The shaft (and fully masked parts): a rectangle, grown analytically.
            _ => {
                let h = 0.5 * w + d;
                let q = [[-d, -h], [l + d, -h], [l + d, h], [-d, h]].map(at_o);
                out.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
            }
        }
    }
}

impl ArrowsResolved {
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

    /// Which of tail, shaft and tip are drawn.
    fn parts(&self) -> [bool; 3] {
        [
            self.taillength > 0.0 && self.tailwidth > 0.0,
            self.shaftwidth > 0.0,
            self.tiplength > 0.0 && self.tipwidth > 0.0,
        ]
    }
}

impl ArrowsState {
    fn len(&self) -> usize {
        self.pos.len().min(self.dir.len())
    }

    /// Start and end points in data space (Makie's `_process_arrow_arguments` for directions).
    fn endpoints(&self, r: &ArrowsResolved) -> Vec<([f64; 2], [f64; 2])> {
        let a = r.align.fraction();
        (0..self.len())
            .map(|i| {
                let (p, mut d) = (self.pos[i], self.dir[i]);
                if r.normalize {
                    let n = (d[0] * d[0] + d[1] * d[1]).sqrt();
                    d = [d[0] / n, d[1] / n];
                }
                let d = [d[0] * r.lengthscale, d[1] * r.lengthscale];
                let s = [p[0] - a * d[0], p[1] - a * d[1]];
                (s, [s[0] + d[0], s[1] + d[1]])
            })
            .collect()
    }

    /// Per-arrow values mapped through the colormap (`None` for unmapped colors).
    fn values(&self, color: &VectorColor) -> Option<Vec<f64>> {
        match color {
            VectorColor::Spec(ColorSpec::Values(v)) => {
                Some((0..self.len()).map(|i| v.get(i).copied().unwrap_or(f64::NAN)).collect())
            }
            VectorColor::Spec(_) => None,
            c => Some(self.dir[..self.len()].iter().map(|d| c.scalar(d[0], d[1]).unwrap_or(f64::NAN)).collect()),
        }
    }

    /// Premultiplied per-arrow colors.
    fn colors(&self, r: &ArrowsResolved, palette: &[Color], cycle: usize) -> Vec<u32> {
        let n = self.len();
        let alpha = r.alpha as f32;
        let premul = |c: Color| c.with_alpha(c.a * alpha).to_premul_u32();
        if let Some(vals) = self.values(&r.color) {
            let map = CpuColormap::new(&r.mapping(), vals.iter().copied());
            return vals.iter().map(|v| map.color(*v).to_premul_u32()).collect();
        }
        match &r.color {
            VectorColor::Spec(ColorSpec::PerPoint(cs)) if cs.len() == n => cs.iter().map(|c| premul(*c)).collect(),
            VectorColor::Spec(spec) => {
                let c = crate::scene::resolve_color(spec, cycle, palette).unwrap_or_else(|| {
                    crate::warn_once("arrows: per-arrow colors must have one entry per arrow; using black");
                    Color::rgb(0.0, 0.0, 0.0)
                });
                vec![premul(c); n]
            }
            _ => vec![premul(Color::rgb(0.0, 0.0, 0.0)); n],
        }
    }
}

impl PlotImpl for ArrowsState {
    fn cycle_group(&self) -> &'static str {
        "arrows"
    }

    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool {
        matches!(self.attrs.color.as_ref().or(theme.arrows.color.as_ref()), Some(VectorColor::Spec(ColorSpec::Auto)))
    }

    fn data_bounds(&self, xs: crate::transform::Scale, ys: crate::transform::Scale) -> Option<[f64; 4]> {
        // Makie: the bounding box of start and end points (not the tip width).
        // (Like `band`'s `direction`, the placement attributes are read from the plot, not the theme.)
        let r = self.attrs.resolve(&ArrowsAttrs::default(), &crate::theme::Globals::default());
        let pts: Vec<[f64; 2]> = self.endpoints(&r).into_iter().flat_map(|(s, e)| [s, e]).collect();
        point_bounds(&pts, xs, ys)
    }

    fn legend_elements(&self, ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        let r = self.attrs.resolve(&ctx.theme.arrows, ctx.g);
        let spec = match &r.color {
            VectorColor::Spec(s) => s.clone(),
            _ => ColorSpec::Values(Arc::new(Vec::new())),
        };
        arrow_legend(
            ctx.color(&spec, false, r.alpha, super::legend_elements::DEFAULT_LINECOLOR),
            r.shaftwidth,
            r.tipwidth,
        )
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.arrows, ctx.g);
        let a = ctx.axis;
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (ctx.data_rev, self.style_rev, ctx.cycle, axis_geometry_key(a)).hash(&mut h);
            h.finish()
        };
        let (g, cycle) = (ctx.g, ctx.cycle);
        let palette = &g.palette;
        let verts = ctx.cache.memo(ctx.uid, 0, key, || {
            let colors = self.colors(&r, palette, cycle);
            let parts = r.parts();
            let mut out = Vec::with_capacity(self.len() * 9);
            for ((s, e), color) in self.endpoints(&r).into_iter().zip(colors) {
                if color >> 24 == 0 {
                    continue;
                }
                let (Some(p0), Some(p1)) = (a.to_units(s[0], s[1]), a.to_units(e[0], e[1])) else { continue };
                let len = (p1[0] - p0[0]).hypot(p1[1] - p0[1]);
                arrow_triangles(&mut out, p0, p1, arrow_metrics(len, &r), parts, r.strokemask, color);
            }
            out
        });
        if verts.is_empty() {
            return;
        }
        let buf = ctx.keyed_buf(0, key, verts);
        ctx.push_figure(Prim::Mesh(MeshPrim { verts: buf }));
    }

    fn pick(&self, ctx: &mut super::pick::PickCtx<'_>) -> Option<super::pick::Hover> {
        let n = self.len();
        let (i, dist, anchor) = ctx.nearest_point(0, &self.pos[..n])?;
        let ([x, y], [u, v]) = (self.pos[i], self.dir[i]);
        let text =
            format!("{}\nu: {}\nv: {}", super::pick::point_text(x, y), super::pick::sig6(u), super::pick::sig6(v));
        Some(super::pick::Hover { dist, anchor, text, ring: Some(12.0), outline: None })
    }

    fn colormapping(&self, theme: &crate::theme::Theme, g: &crate::theme::Globals) -> Option<super::ResolvedColormap> {
        let r = self.attrs.resolve(&theme.arrows, g);
        let Some(vals) = self.values(&r.color) else {
            return Some(super::ResolvedColormap::unmapped(r.colormap, r.alpha));
        };
        let [lo, hi] =
            r.colorrange.unwrap_or_else(|| ValueEncoding::new(crate::data::finite_extrema(vals)).auto_range());
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

/// A legend entry for arrows: a line of the shaft width with an arrowhead at its center.
pub(crate) fn arrow_legend(color: Color, linewidth: f64, headsize: f64) -> Vec<super::LegendElement> {
    vec![
        super::LegendElement::Line { color, linewidth, linestyle: Linestyle::Solid },
        super::LegendElement::Marker {
            color,
            marker: Marker::RTriangle,
            markersize: headsize,
            strokecolor: Color::TRANSPARENT,
            strokewidth: 0.0,
        },
    ]
}

impl Arrows {
    fn with_attrs(&self, f: impl FnOnce(&mut ArrowsAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Arrows(s) = &mut p.kind {
                f(&mut s.attrs);
                s.style_rev += 1;
            }
        });
    }

    pub(crate) fn create(ax: &crate::Axis, pos: Vec<[f64; 2]>, dir: Vec<[f64; 2]>) -> Arrows {
        let st = ArrowsState { pos: Arc::new(pos), dir: Arc::new(dir), attrs: ArrowsAttrs::default(), style_rev: 0 };
        let id = add_to_axis(ax, PlotKind::Arrows(st));
        Arrows { sh: ax.sh.clone(), id }
    }

    /// Colors each arrow by `f(u, v)` of its direction, mapped through the colormap
    /// (`color(Magnitude)` is `color_fn(|u, v| u.hypot(v))`).
    pub fn color_fn(&self, f: impl Fn(f64, f64) -> f64 + Send + Sync + 'static) -> Arrows {
        self.color(VectorColor::Func(Arc::new(f)))
    }

    fn replace(&self, pos: Vec<[f64; 2]>, dir: Vec<[f64; 2]>) {
        self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            if let PlotKind::Arrows(s) = &mut p.kind {
                s.pos = Arc::new(pos);
                s.dir = Arc::new(dir);
                p.data_rev += 1;
            }
        });
    }

    /// Replaces positions and directions (all four must have equal length).
    #[track_caller]
    pub fn set_data(&self, x: impl Data1D, y: impl Data1D, u: impl Data1D, v: impl Data1D) -> Arrows {
        let (pos, dir) = components("Arrows::set_data", x, y, u, v);
        self.replace(pos, dir);
        self.clone()
    }

    /// Replaces positions and directions given as points (equal lengths).
    #[track_caller]
    pub fn set_points(&self, points: impl PointData, dirs: impl PointData) -> Arrows {
        let (pos, dir) = (points.to_points(), dirs.to_points());
        assert!(pos.len() == dir.len(), "Arrows::set_points: {} points but {} directions", pos.len(), dir.len());
        self.replace(pos, dir);
        self.clone()
    }

    /// Re-evaluates the directions at the current positions with `f(x, y) -> (u, v)`.
    pub fn set_fn(&self, f: impl Fn(f64, f64) -> (f64, f64)) -> Arrows {
        let pos = match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Arrows(s)) => s.pos.clone(),
            _ => return self.clone(),
        };
        let dir = eval_dirs(&pos, f);
        self.replace(pos.to_vec(), dir);
        self.clone()
    }

    /// Number of arrows.
    pub fn len(&self) -> usize {
        match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Arrows(s)) => s.len(),
            _ => 0,
        }
    }

    /// Whether there are no arrows.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[track_caller]
fn components(
    what: &str,
    x: impl Data1D,
    y: impl Data1D,
    u: impl Data1D,
    v: impl Data1D,
) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
    let (x, y, u, v) = (x.to_vec_f64(), y.to_vec_f64(), u.to_vec_f64(), v.to_vec_f64());
    assert!(
        x.len() == u.len(),
        "{what}: {} positions but {} directions (x, y, u and v must have equal length)",
        x.len(),
        u.len()
    );
    (zip_xy(what, x, y), zip_xy(what, u, v))
}

/// The grid `xs × ys` (x fastest, Makie's `vec(Point2.(x, y'))`).
fn grid(xs: impl Data1D, ys: impl Data1D) -> Vec<[f64; 2]> {
    let (xs, ys) = (xs.to_vec_f64(), ys.to_vec_f64());
    ys.iter().flat_map(|y| xs.iter().map(move |x| [*x, *y])).collect()
}

fn eval_dirs(pos: &[[f64; 2]], f: impl Fn(f64, f64) -> (f64, f64)) -> Vec<[f64; 2]> {
    pos.iter()
        .map(|p| {
            let (u, v) = f(p[0], p[1]);
            [u, v]
        })
        .collect()
}

impl crate::Axis {
    /// Makie's `arrows2d!(ax, x, y, u, v)` (quiver): arrows at `(x[i], y[i])` along `(u[i], v[i])`.
    #[doc(alias = "quiver")]
    #[track_caller]
    pub fn arrows(&self, x: impl Data1D, y: impl Data1D, u: impl Data1D, v: impl Data1D) -> Arrows {
        let (pos, dir) = components("arrows", x, y, u, v);
        Arrows::create(self, pos, dir)
    }

    /// Makie's `arrows2d!(ax, points, directions)`.
    #[track_caller]
    pub fn arrows_points(&self, points: impl PointData, dirs: impl PointData) -> Arrows {
        let (pos, dir) = (points.to_points(), dirs.to_points());
        assert!(pos.len() == dir.len(), "arrows_points: {} points but {} directions", pos.len(), dir.len());
        Arrows::create(self, pos, dir)
    }

    /// Makie's `arrows2d!(ax, xs, ys, f)`: an arrow at every grid point `(x, y)` of `xs × ys`
    /// along `f(x, y) -> (u, v)`.
    pub fn arrows_fn(&self, xs: impl Data1D, ys: impl Data1D, f: impl Fn(f64, f64) -> (f64, f64)) -> Arrows {
        let pos = grid(xs, ys);
        let dir = eval_dirs(&pos, f);
        Arrows::create(self, pos, dir)
    }
}

impl crate::GridPosition {
    /// Makie's `arrows2d(fig[r, c], x, y, u, v)`: a new Axis at this position with arrows.
    #[track_caller]
    pub fn arrows(&self, x: impl Data1D, y: impl Data1D, u: impl Data1D, v: impl Data1D) -> Arrows {
        crate::Axis::new(self.clone()).arrows(x, y, u, v)
    }

    /// Makie's `arrows2d(fig[r, c], points, directions)`.
    #[track_caller]
    pub fn arrows_points(&self, points: impl PointData, dirs: impl PointData) -> Arrows {
        crate::Axis::new(self.clone()).arrows_points(points, dirs)
    }

    /// Makie's `arrows2d(fig[r, c], xs, ys, f)`: arrows on the grid `xs × ys`.
    pub fn arrows_fn(&self, xs: impl Data1D, ys: impl Data1D, f: impl Fn(f64, f64) -> (f64, f64)) -> Arrows {
        crate::Axis::new(self.clone()).arrows_fn(xs, ys, f)
    }
}

/// Makie's `arrows2d(x, y, u, v)` (quiver): a new Figure and Axis with arrows.
#[doc(alias = "quiver")]
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn arrows(x: impl Data1D, y: impl Data1D, u: impl Data1D, v: impl Data1D) -> Arrows {
    crate::Figure::new().at(1, 1).arrows(x, y, u, v)
}

/// Makie's `arrows2d(points, directions)`: a new Figure and Axis with arrows.
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn arrows_points(points: impl PointData, dirs: impl PointData) -> Arrows {
    crate::Figure::new().at(1, 1).arrows_points(points, dirs)
}

/// Makie's `arrows2d(xs, ys, f)`: a new Figure and Axis with arrows on the grid `xs × ys`.
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn arrows_fn(xs: impl Data1D, ys: impl Data1D, f: impl Fn(f64, f64) -> (f64, f64)) -> Arrows {
    crate::Figure::new().at(1, 1).arrows_fn(xs, ys, f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::scene::{SceneCache, build};

    fn resolved() -> ArrowsResolved {
        ArrowsAttrs::default().resolve(&ArrowsAttrs::default(), &crate::theme::Globals::default())
    }

    #[test]
    fn metrics_follow_makie() {
        let r = resolved();
        // Long arrows keep the default shape and stretch the shaft.
        assert_eq!(arrow_metrics(100.0, &r), [0.0, 14.0, 92.0, 3.0, 8.0, 14.0]);
        // Shorter than tip + minshaftlength (18): the whole arrow scales down.
        assert_eq!(arrow_metrics(9.0, &r), [0.0, 7.0, 5.0, 1.5, 4.0, 7.0]);
        let mut r2 = resolved();
        r2.shaftlength = Some(20.0);
        let m = arrow_metrics(56.0, &r2);
        assert!((m[2] - 40.0).abs() < 1e-12 && (m[5] - 28.0).abs() < 1e-12, "{m:?}");
    }

    #[test]
    fn triangles_have_makie_widths() {
        let r = resolved();
        let mut v = Vec::new();
        arrow_triangles(&mut v, [0.0, 0.0], [100.0, 0.0], arrow_metrics(100.0, &r), r.parts(), 0.0, 1);
        assert_eq!(v.len(), 9);
        let ys: Vec<f32> = v.iter().map(|q| q.pos[1]).collect();
        assert_eq!(ys[..6].iter().fold(0.0f32, |a, y| a.max(y.abs())), 1.5);
        assert_eq!(v[7].pos, [100.0, 0.0], "the tip apex is the end point");
        // With the stroke mask the shaft keeps its width (3 - 0.75 + 2 × 0.375) and grows 0.375
        // at its back; the tip apex moves forward by 0.375 / sin(half apex angle).
        v.clear();
        arrow_triangles(&mut v, [0.0, 0.0], [100.0, 0.0], arrow_metrics(100.0, &r), r.parts(), 0.75, 1);
        assert_eq!(v[0].pos, [-0.375, -1.5]);
        let half = (6.625f64).atan2(8.0);
        assert!((v[7].pos[0] as f64 - (100.0 + 0.375 / half.sin())).abs() < 1e-4, "{:?}", v[7].pos);
        // A tail adds a fan of five triangles.
        let mut r = resolved();
        r.taillength = 8.0;
        v.clear();
        arrow_triangles(&mut v, [0.0, 0.0], [100.0, 0.0], arrow_metrics(100.0, &r), r.parts(), 0.75, 1);
        assert_eq!(v.len(), 24);
    }

    #[test]
    fn autolimits_cover_tails_and_tips() {
        let ar = arrows_points([[0.0, 0.0]], [[2.0, 1.0]]).align(ArrowAlign::Center);
        let g = ar.axis().geometry().unwrap();
        // Start (-1, -0.5), end (1, 0.5), plus Makie's 5 % margins.
        let want = [-1.1, 1.1, -0.55, 0.55];
        assert!(g.limits.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9), "{:?}", g.limits);
        ar.align(ArrowAlign::Tip).lengthscale(2.0).normalize(true);
        let g = ar.axis().geometry().unwrap();
        let d = [2.0 / 5f64.sqrt(), 1.0 / 5f64.sqrt()];
        assert!((g.limits[0] - (-2.0 * d[0] * 1.05)).abs() < 1e-9, "{:?}", g.limits);
        assert!((g.limits[3] - 0.05 * 2.0 * d[1]).abs() < 1e-9, "{:?}", g.limits);
    }

    /// The figure-space arrow meshes of the figure.
    fn meshes(fig: &Figure, cache: &mut SceneCache) -> Vec<Arc<Vec<MeshVertex>>> {
        let (dl, _) = build(&fig.sh.snapshot(), None, cache);
        dl.items
            .iter()
            .filter_map(|i| match (&i.prim, i.space) {
                (Prim::Mesh(m), crate::scene::drawlist::Space::Figure) if i.clip.is_some() => {
                    Some(m.verts.data.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn mesh_is_memoized_until_the_axis_moves() {
        let ar = arrows([0.0, 1.0], [0.0, 1.0], [1.0, 1.0], [0.0, 1.0]);
        let fig = ar.figure();
        let mut cache = SceneCache::new();
        let a = meshes(&fig, &mut cache);
        let b = meshes(&fig, &mut cache);
        assert_eq!(a.len(), 1);
        assert!(Arc::ptr_eq(&a[0], &b[0]));
        // Zooming out shortens the arrows on screen but keeps the shaft width (3 units).
        crate::testing::set_interactive_limits(&ar.axis(), [-10.0, 10.0, -10.0, 10.0]);
        let c = meshes(&fig, &mut cache);
        assert!(!Arc::ptr_eq(&a[0], &c[0]));
        let ys: Vec<f32> = c[0][..6].iter().map(|v| v.pos[1]).collect();
        let w = ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min);
        assert!((w - 3.0).abs() < 1e-3, "shaft width {w}");
        ar.shaftwidth(5);
        assert!(!Arc::ptr_eq(&c[0], &meshes(&fig, &mut cache)[0]));
    }

    #[test]
    fn colors_and_colormapping() {
        let ar = arrows([0.0, 0.0], [0.0, 1.0], [3.0, 0.0], [4.0, 1.0]).color(Magnitude);
        let m = ar.colormapping().unwrap();
        assert!(m.mapped && m.colorrange == (1.0, 5.0), "{m:?}");
        ar.color_fn(|u, _| u).colorrange((0, 10));
        assert_eq!(ar.colormapping().unwrap().colorrange, (0.0, 10.0));
        ar.color(RED);
        assert!(!ar.colormapping().unwrap().mapped);
        // Legend: a line with an arrowhead in the arrow color.
        let st = ar.sh.snapshot();
        let g = st.theme.globals();
        let ctx = super::super::legend_elements::LegendCtx { theme: &st.theme, g: &g, cycle: 0 };
        let imp = st.plot(ar.id).unwrap().kind.imp();
        match imp.legend_elements(&ctx).as_slice() {
            [LegendElement::Line { color, .. }, LegendElement::Marker { marker: Marker::RTriangle, .. }] => {
                assert_eq!(*color, RED)
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn cpu_colormap_clips_like_the_gpu() {
        let cm = Colormap::VIRIDIS;
        let m = MappingAttrs {
            colormap: &cm,
            colorrange: None,
            lowclip: Some(RED),
            highclip: None,
            nan_color: Color::TRANSPARENT,
            alpha: 0.5,
        };
        let map = CpuColormap::new(&m, [1.0, 3.0]);
        assert_eq!(map.color(0.0), RED.with_alpha(0.5));
        assert_eq!(map.color(9.0), cm.last().with_alpha(0.5));
        assert_eq!(map.color(1.0), cm.first().with_alpha(0.5));
        assert_eq!(map.color(f64::NAN).a, 0.0);
    }
}
