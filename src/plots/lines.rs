//! `lines`: a polyline through points; NaN values break it into separate runs.

use super::{ColorSpec, PlotImpl, PlotKind, add_to_axis, is_auto, plot_common, zip_xy};
use crate::attrs::attributes;
use crate::color::{Color, Colormap, MappingAttrs, encoded_values};
use crate::data::points::Points;
use crate::data::{Data1D, PointData};
use crate::figure::{FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::scene::drawlist::{Buf, BufKey, LinesPrim, Prim, PrimColor};
use crate::style::{JoinStyle, LineCap, Linestyle};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// A line plot handle (Makie's `Lines`). Besides styling, it supports live data: [`set_data`],
/// [`push`] and [`extend`] append in O(1) amortized time under the figure lock and upload only the
/// new points to the GPU.
///
/// ```no_run
/// use ezviz::prelude::*;
/// let t: Vec<f64> = (0..200).map(|i| i as f64 / 20.0).collect();
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// lines!(ax, &t, t.iter().map(|t| t.sin()); label = "sin", linewidth = 2);
/// ax.lines(&t, t.iter().map(|t| t.cos())).linestyle(Linestyle::Dash);
/// fig.save("lines.png").unwrap();
/// ```
///
/// [`set_data`]: Lines::set_data
/// [`push`]: Lines::push
/// [`extend`]: Lines::extend
#[derive(Clone)]
pub struct Lines {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct LinesState {
    pub pts: Points,
    pub attrs: LinesAttrs,
    /// Bumped by every attribute change (keys derived GPU buffers such as per-point colors).
    pub style_rev: u64,
}

attributes! {
    Lines(LinesAttrs, LinesResolved, LinesTheme) via with_attrs {
        /// A color, `Cycled(i)`, per-point colors, or per-point values mapped through the colormap
        /// (interpolated along each segment).
        color: ColorSpec = |_| ColorSpec::Auto, STYLE;
        /// Line width in units (Makie default 1.5).
        linewidth: f64 = |g| g.linewidth, STYLE;
        /// Dash pattern (Makie's `linestyle`; `Solid` is Makie's `nothing`).
        linestyle: Linestyle = |_| Linestyle::Solid, STYLE;
        /// Shape of the line ends.
        linecap: LineCap = |_| LineCap::Butt, STYLE;
        /// Shape of the corners.
        joinstyle: JoinStyle = |_| JoinStyle::Miter, STYLE;
        /// Miter joints sharper than this angle (radians) are beveled (Makie default π/3).
        miter_limit: f64 = |_| std::f64::consts::FRAC_PI_3, STYLE;
        /// Opacity multiplier.
        alpha: f64 = |_| 1.0, STYLE;
        /// Colormap for `color = values` (default viridis).
        colormap: Colormap = |_| Colormap::VIRIDIS, STYLE;
        /// `(lo, hi)` mapped to the colormap ends; default: the finite extrema of the values.
        colorrange: Option<[f64; 2]> = |_| None, STYLE;
        /// Color for values below the colorrange (default: the first colormap color).
        lowclip: Option<Color> = |_| None, STYLE;
        /// Color for values above the colorrange (default: the last colormap color).
        highclip: Option<Color> = |_| None, STYLE;
        /// Color for NaN values (default transparent).
        nan_color: Color = |_| Color::TRANSPARENT, STYLE;
    }
}

plot_common!(Lines);

/// Resolved line styling shared by `Lines` and `ScatterLines`.
pub(crate) struct LineStyle<'a> {
    pub color: &'a ColorSpec,
    pub linewidth: f64,
    pub linestyle: &'a Linestyle,
    pub linecap: LineCap,
    pub joinstyle: JoinStyle,
    pub miter_limit: f64,
    pub alpha: f64,
}

impl LinesResolved {
    fn style(&self) -> LineStyle<'_> {
        LineStyle {
            color: &self.color,
            linewidth: self.linewidth,
            linestyle: &self.linestyle,
            linecap: self.linecap,
            joinstyle: self.joinstyle,
            miter_limit: self.miter_limit,
            alpha: self.alpha,
        }
    }
}

/// Emits a polyline through `pts` (part 0: points, part 1: colors). Returns the point buffer and
/// the line's color so companions (scatterlines markers) can share them.
pub(crate) fn emit_line(
    ctx: &mut PlotCtx<'_>,
    pts: &Points,
    s: &LineStyle<'_>,
    style_rev: u64,
) -> Option<(Buf<[f32; 2]>, PrimColor)> {
    emit_line_mapped(ctx, pts, s, style_rev, None)
}

/// [`emit_line`] with explicit colormapping attributes for `color = values` (`None`: viridis over
/// the finite extrema).
pub(crate) fn emit_line_mapped(
    ctx: &mut PlotCtx<'_>,
    pts: &Points,
    s: &LineStyle<'_>,
    style_rev: u64,
    map: Option<&MappingAttrs<'_>>,
) -> Option<(Buf<[f32; 2]>, PrimColor)> {
    if pts.is_empty() {
        return None;
    }
    let buf = ctx.local_points_append(0, pts);
    let color = prim_color_mapped(ctx, s.color, s.alpha as f32, pts.len(), 1, style_rev, map);
    if pts.len() >= 2 {
        ctx.push_data(Prim::Lines(LinesPrim {
            pts: buf.clone(),
            color: color.clone(),
            width: s.linewidth as f32,
            pattern: s.linestyle.pattern(),
            cap: s.linecap,
            join: s.joinstyle,
            miter_limit: s.miter_limit as f32,
            segments: false,
            closed: pts.is_closed(),
            append: true,
        }));
    }
    Some((buf, color))
}

/// Lowers a color spec for `n` points: a uniform color, premultiplied per-point colors, or values
/// mapped through viridis over their finite extrema. Per-point buffers are cached on the GPU
/// under `part` until the data or the style changes.
pub(crate) fn prim_color(
    ctx: &PlotCtx<'_>,
    spec: &ColorSpec,
    alpha: f32,
    n: usize,
    part: u8,
    style_rev: u64,
) -> PrimColor {
    prim_color_mapped(ctx, spec, alpha, n, part, style_rev, None)
}

/// [`prim_color`] with explicit colormapping attributes (colormap, colorrange, lowclip, highclip,
/// nan_color) for `ColorSpec::Values`; `None` uses viridis over the finite extrema.
pub(crate) fn prim_color_mapped(
    ctx: &PlotCtx<'_>,
    spec: &ColorSpec,
    alpha: f32,
    n: usize,
    part: u8,
    style_rev: u64,
    map: Option<&MappingAttrs<'_>>,
) -> PrimColor {
    if let Some(c) = ctx.solid_color(spec, false) {
        return PrimColor::Uniform(c.with_alpha(c.a * alpha));
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (ctx.data_rev, style_rev, part).hash(&mut h);
    let key = Some(BufKey { uid: ctx.uid, part, rev: h.finish() });
    match spec {
        ColorSpec::PerPoint(cs) if cs.len() == n => {
            let data = cs.iter().map(|c| c.with_alpha(c.a * alpha).to_premul_u32()).collect();
            PrimColor::PerElement(Buf { key, data: Arc::new(data) })
        }
        ColorSpec::Values(v) if v.len() == n => {
            let e = encoded_values(v);
            let viridis = Colormap::VIRIDIS;
            let default = MappingAttrs {
                colormap: &viridis,
                colorrange: None,
                lowclip: None,
                highclip: None,
                nan_color: Color::TRANSPARENT,
                alpha: 1.0,
            };
            let m = map.unwrap_or(&default);
            let mapping = MappingAttrs { alpha: alpha as f64, ..*m }.mapping(&e.enc);
            PrimColor::Values(Buf { key: Some(BufKey { uid: ctx.uid, part, rev: e.rev }), data: e.data }, mapping)
        }
        _ => {
            crate::warn_once("per-point line/marker colors must have one entry per point; using the palette color");
            let c = ctx.g.palette[ctx.cycle % ctx.g.palette.len().max(1)];
            PrimColor::Uniform(c.with_alpha(c.a * alpha))
        }
    }
}

impl PlotImpl for LinesState {
    fn cycle_group(&self) -> &'static str {
        "lines"
    }

    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool {
        is_auto(self.attrs.color.as_ref(), theme.lines.color.as_ref())
    }

    fn data_bounds(&self, xs: crate::transform::Scale, ys: crate::transform::Scale) -> Option<[f64; 4]> {
        self.pts.bounds(xs, ys)
    }

    fn legend_elements(&self, ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        let r = self.attrs.resolve(&ctx.theme.lines, ctx.g);
        vec![super::LegendElement::Line {
            color: ctx.color(&r.color, false, r.alpha, super::legend_elements::DEFAULT_LINECOLOR),
            linewidth: r.linewidth,
            linestyle: r.linestyle,
        }]
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.lines, ctx.g);
        let map = MappingAttrs {
            colormap: &r.colormap,
            colorrange: r.colorrange,
            lowclip: r.lowclip,
            highclip: r.highclip,
            nan_color: r.nan_color,
            alpha: r.alpha,
        };
        emit_line_mapped(ctx, &self.pts, &r.style(), self.style_rev, Some(&map));
    }

    fn pick(&self, ctx: &mut super::pick::PickCtx<'_>) -> Option<super::pick::Hover> {
        let (dist, anchor, [x, y]) = ctx.nearest_on_polyline(self.pts.iter())?;
        Some(super::pick::Hover { dist, anchor, text: super::pick::point_text(x, y), ring: None, outline: None })
    }
}

/// Live-data methods shared by plots backed by append-only `Points` (`Lines`, `ScatterLines`).
macro_rules! live_points {
    ($Handle:ident, $Variant:ident, $name:literal) => {
        impl $Handle {
            fn with_points(&self, f: impl FnOnce(&mut $crate::data::points::Points)) {
                self.with_slot($crate::figure::Dirty::DATA | $crate::figure::Dirty::LIMITS, |p| {
                    if let $crate::plots::PlotKind::$Variant(s) = &mut p.kind {
                        f(&mut s.pts);
                        p.data_rev += 1;
                    }
                });
            }

            /// Replaces the points (the lengths of x and y must match).
            #[track_caller]
            pub fn set_data(&self, x: impl $crate::Data1D, y: impl $crate::Data1D) -> Self {
                let pts = $crate::data::points::Points::new($crate::plots::zip_xy(
                    concat!($name, "::set_data"),
                    x.to_vec_f64(),
                    y.to_vec_f64(),
                ));
                self.with_points(move |p| *p = pts);
                self.clone()
            }

            /// Replaces the points: `&[[x, y], ..]`, `&[(x, y), ..]`, or y values with x = 1..=n.
            pub fn set_points(&self, p: impl $crate::PointData) -> Self {
                let pts = $crate::data::points::Points::new(p.to_points());
                self.with_points(move |p| *p = pts);
                self.clone()
            }

            /// Appends one point: O(1) amortized, and only the new point is uploaded to the GPU.
            pub fn push(&self, x: impl $crate::Scalar, y: impl $crate::Scalar) -> Self {
                let q = [x.to_f64(), y.to_f64()];
                self.with_points(move |p| p.push(q));
                self.clone()
            }

            /// Appends points (the lengths of xs and ys must match).
            #[track_caller]
            pub fn extend(&self, xs: impl $crate::Data1D, ys: impl $crate::Data1D) -> Self {
                let v = $crate::plots::zip_xy(concat!($name, "::extend"), xs.to_vec_f64(), ys.to_vec_f64());
                self.with_points(move |p| p.extend_from_slice(&v));
                self.clone()
            }

            /// Removes every point.
            pub fn clear(&self) -> Self {
                self.with_points(|p| p.clear());
                self.clone()
            }

            /// Number of points.
            pub fn len(&self) -> usize {
                match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
                    Some($crate::plots::PlotKind::$Variant(s)) => s.pts.len(),
                    _ => 0,
                }
            }

            /// Whether there are no points.
            pub fn is_empty(&self) -> bool {
                self.len() == 0
            }
        }
    };
}
pub(crate) use live_points;

live_points!(Lines, Lines, "Lines");

impl Lines {
    fn with_attrs(&self, f: impl FnOnce(&mut LinesAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Lines(s) = &mut p.kind {
                f(&mut s.attrs);
                s.style_rev += 1;
            }
        });
    }

    pub(crate) fn create(ax: &crate::Axis, pts: Vec<[f64; 2]>) -> Lines {
        let st = LinesState { pts: Points::new(pts), attrs: LinesAttrs::default(), style_rev: 0 };
        let id = add_to_axis(ax, PlotKind::Lines(st));
        Lines { sh: ax.sh.clone(), id }
    }
}

impl crate::Axis {
    /// Makie's `lines!(ax, x, y)`.
    #[track_caller]
    pub fn lines(&self, x: impl Data1D, y: impl Data1D) -> Lines {
        Lines::create(self, zip_xy("lines", x.to_vec_f64(), y.to_vec_f64()))
    }

    /// Makie's `lines!(ax, points)` or `lines!(ax, y)` (x = 1..=n).
    pub fn lines_points(&self, p: impl PointData) -> Lines {
        Lines::create(self, p.to_points())
    }
}

impl crate::GridPosition {
    /// Makie's `lines(fig[r, c], x, y)`: a new Axis at this position with a line plot.
    #[track_caller]
    pub fn lines(&self, x: impl Data1D, y: impl Data1D) -> Lines {
        crate::Axis::new(self.clone()).lines(x, y)
    }

    /// Makie's `lines(fig[r, c], points)`: a new Axis at this position with a line plot.
    pub fn lines_points(&self, p: impl PointData) -> Lines {
        crate::Axis::new(self.clone()).lines_points(p)
    }
}

/// Makie's `lines(x, y)`: a new Figure and Axis with a line plot. Returns the plot handle; call
/// `.save(..)` or `.show()` on it.
///
/// ```no_run
/// let x = ezviz::linspace(0.0, 10.0, 200);
/// ezviz::lines(&x, x.iter().map(|v| v.sin())).save("sin.png").unwrap();
/// ```
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn lines(x: impl Data1D, y: impl Data1D) -> Lines {
    crate::Figure::new().at(1, 1).lines(x, y)
}

/// Makie's `lines(points)` / `lines(y)`: a new Figure and Axis with a line plot.
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn lines_points(p: impl PointData) -> Lines {
    crate::Figure::new().at(1, 1).lines_points(p)
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::scene::drawlist::{Prim, PrimColor};
    use crate::scene::{SceneCache, build};

    /// The `color = values` mapping of the figure's first line.
    fn line_mapping(fig: &Figure) -> crate::scene::drawlist::ColorMapping {
        let (dl, _) = build(&fig.sh.snapshot(), None, &mut SceneCache::new());
        dl.items
            .iter()
            .find_map(|i| match &i.prim {
                Prim::Lines(l) => match &l.color {
                    PrimColor::Values(_, m) => Some(m.clone()),
                    _ => None,
                },
                _ => None,
            })
            .expect("a value-colored line")
    }

    #[test]
    fn colormap_attributes() {
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        let l = ax.lines([0.0, 1.0, 2.0], [0.0, 1.0, 0.0]).color(vec![0.0, 5.0, 10.0]);
        // Default: viridis over the extrema; encoded values span -1..1.
        let m = line_mapping(&fig);
        assert_eq!(m.lut.first(), Colormap::VIRIDIS.lut().first());
        assert!((m.range[0] + 1.0).abs() < 1e-6 && (m.range[1] - 1.0).abs() < 1e-6, "{:?}", m.range);
        l.colormap(Colormap::MAGMA).colorrange((0, 20)).lowclip(RED).highclip(BLUE).nan_color(GREEN).alpha(0.5);
        let m = line_mapping(&fig);
        assert_eq!(m.lut.last(), Colormap::MAGMA.lut().last());
        // 0 -> -1, 10 -> 1, so 20 -> 3.
        assert!((m.range[0] + 1.0).abs() < 1e-6 && (m.range[1] - 3.0).abs() < 1e-6, "{:?}", m.range);
        assert_eq!((m.lowclip, m.highclip, m.nan_color), (Some(RED), Some(BLUE), GREEN));
        assert_eq!(m.alpha, 0.5);
    }

    #[test]
    fn pick_closest_point_on_polyline() {
        let l = lines([0.0, 10.0, 10.0], [0.0, 0.0, 10.0]);
        l.axis().limits(0.0, 10.0, 0.0, 10.0);
        let st = l.sh.snapshot();
        let (_, axes) = build(&st, None, &mut SceneCache::new());
        let a = &axes[0];
        let g = st.theme.globals();
        let mut cache = crate::plots::pick::PickCache::default();
        let p = st.plots[0].as_ref().unwrap();
        let mut at = |cursor: [f64; 2]| {
            let mut ctx = crate::plots::pick::PickCtx {
                axis: a,
                cursor,
                radius: 10.0,
                uid: p.uid,
                data_rev: p.data_rev,
                theme: &st.theme,
                g: &g,
                cache: &mut cache,
            };
            p.kind.imp().pick(&mut ctx)
        };
        // 4 units above the middle of the first (bottom) segment, at x = 5.
        let on = a.to_units(5.0, 0.0).unwrap();
        let h = at([on[0], on[1] - 4.0]).expect("hovered");
        assert!(
            (h.dist - 4.0).abs() < 1e-9 && (h.anchor[0] - on[0]).abs() < 1e-9 && (h.anchor[1] - on[1]).abs() < 1e-9
        );
        assert_eq!(h.text, "x: 5\ny: 0");
        assert!(at([on[0], on[1] - 20.0]).is_none(), "outside the radius");
    }
}
