//! `scatterlines`: a polyline with markers at its points.

use super::lines::{LineStyle, emit_line_mapped, live_points, prim_color_mapped};
use super::{ColorSpec, PlotImpl, PlotKind, add_to_axis, is_auto, plot_common, zip_xy};
use crate::attrs::attributes;
use crate::color::{Color, Colormap, MappingAttrs, encoded_values};
use crate::data::points::Points;
use crate::data::{Data1D, PointData};
use crate::figure::{FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::scene::drawlist::{MarkersPrim, Prim};
use crate::style::{JoinStyle, LineCap, Linestyle, Marker};
use std::sync::Arc;

/// A scatterlines plot handle (Makie's `ScatterLines`): lines plus markers, with the same live-data
/// methods as [`Lines`](crate::Lines).
///
/// ```no_run
/// use sciplot::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// scatterlines!(ax, [1, 2, 3, 4], [1.0, 3.0, 2.0, 4.0]; markersize = 12, markercolor = RED);
/// fig.save("scatterlines.png").unwrap();
/// ```
#[derive(Clone)]
pub struct ScatterLines {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct ScatterLinesState {
    pub pts: Points,
    pub attrs: ScatterLinesAttrs,
    pub style_rev: u64,
}

attributes! {
    ScatterLines(ScatterLinesAttrs, ScatterLinesResolved, ScatterLinesTheme) via with_attrs {
        /// Color of the line and, by default, of the markers.
        color: ColorSpec = |_| ColorSpec::Auto, STYLE;
        /// Line width in units (Makie default 1.5).
        linewidth: f64 = |g| g.linewidth, STYLE;
        linestyle: Linestyle = |_| Linestyle::Solid, STYLE;
        linecap: LineCap = |_| LineCap::Butt, STYLE;
        joinstyle: JoinStyle = |_| JoinStyle::Miter, STYLE;
        /// Miter joints sharper than this angle (radians) are beveled (Makie default π/3).
        miter_limit: f64 = |_| std::f64::consts::FRAC_PI_3, STYLE;
        /// Opacity multiplier for line and markers.
        alpha: f64 = |_| 1.0, STYLE;
        marker: Marker = |_| Marker::Circle, STYLE;
        /// Marker size in units (Makie default 9).
        markersize: f64 = |g| g.markersize, STYLE;
        /// Marker color; `ColorSpec::Auto` (the default) uses the line color.
        markercolor: ColorSpec = |_| ColorSpec::Auto, STYLE;
        strokecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        strokewidth: f64 = |_| 0.0, STYLE;
        /// Colormap for `color = values` / `markercolor = values` (default viridis).
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

plot_common!(ScatterLines);
super::color_mapped!(ScatterLines);
live_points!(ScatterLines, ScatterLines, "ScatterLines");

impl ScatterLinesResolved {
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
}

impl PlotImpl for ScatterLinesState {
    fn cycle_group(&self) -> &'static str {
        "scatterlines"
    }

    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool {
        is_auto(self.attrs.color.as_ref(), theme.scatterlines.color.as_ref())
    }

    fn data_bounds(&self, xs: crate::transform::Scale, ys: crate::transform::Scale) -> Option<[f64; 4]> {
        self.pts.bounds(xs, ys)
    }

    fn legend_elements(&self, ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        use super::legend_elements::{DEFAULT_LINECOLOR, DEFAULT_MARKERCOLOR};
        let r = self.attrs.resolve(&ctx.theme.scatterlines, ctx.g);
        let line = ctx.color(&r.color, false, r.alpha, DEFAULT_LINECOLOR);
        let marker = match &r.markercolor {
            ColorSpec::Auto => ctx.color(&r.color, false, r.alpha, DEFAULT_MARKERCOLOR),
            spec => ctx.color(spec, false, r.alpha, DEFAULT_MARKERCOLOR),
        };
        vec![
            super::LegendElement::Line { color: line, linewidth: r.linewidth, linestyle: r.linestyle },
            super::LegendElement::Marker {
                color: marker,
                marker: r.marker,
                markersize: r.markersize,
                strokecolor: r.strokecolor.with_alpha(r.strokecolor.a * r.alpha as f32),
                strokewidth: r.strokewidth,
            },
        ]
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.scatterlines, ctx.g);
        let style = LineStyle {
            color: &r.color,
            linewidth: r.linewidth,
            linestyle: &r.linestyle,
            linecap: r.linecap,
            joinstyle: r.joinstyle,
            miter_limit: r.miter_limit,
            alpha: r.alpha,
        };
        let map = r.mapping();
        let Some((pos, line_color)) = emit_line_mapped(ctx, &self.pts, &style, self.style_rev, Some(&map)) else {
            return;
        };
        let alpha = r.alpha as f32;
        let color = match &r.markercolor {
            ColorSpec::Auto => line_color,
            spec => prim_color_mapped(ctx, spec, alpha, self.pts.len(), 3, self.style_rev, Some(&map)),
        };
        ctx.push_data(Prim::Markers(MarkersPrim {
            pos,
            color,
            size: r.markersize as f32,
            sizes: None,
            marker: r.marker,
            stroke_color: r.strokecolor.with_alpha(r.strokecolor.a * alpha),
            stroke_width: r.strokewidth as f32,
            rotation: 0.0,
        }));
    }

    fn colormapping(&self, theme: &crate::theme::Theme, g: &crate::theme::Globals) -> Option<super::ResolvedColormap> {
        let r = self.attrs.resolve(&theme.scatterlines, g);
        // The line's values, else the markers' (both share the colormap attributes).
        let values = match (&r.color, &r.markercolor) {
            (ColorSpec::Values(v), _) | (_, ColorSpec::Values(v)) => v,
            _ => return Some(super::ResolvedColormap::unmapped(r.colormap, r.alpha)),
        };
        let [lo, hi] = r.colorrange.unwrap_or_else(|| encoded_values(values).enc.auto_range());
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

impl ScatterLines {
    fn with_attrs(&self, f: impl FnOnce(&mut ScatterLinesAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::ScatterLines(s) = &mut p.kind {
                f(&mut s.attrs);
                s.style_rev += 1;
            }
        });
    }

    pub(crate) fn create(ax: &crate::Axis, pts: Vec<[f64; 2]>) -> ScatterLines {
        let st = ScatterLinesState { pts: Points::new(pts), attrs: ScatterLinesAttrs::default(), style_rev: 0 };
        let id = add_to_axis(ax, PlotKind::ScatterLines(st));
        ScatterLines { sh: ax.sh.clone(), id }
    }
}

impl crate::Axis {
    /// Makie's `scatterlines!(ax, x, y)`.
    #[track_caller]
    pub fn scatterlines(&self, x: impl Data1D, y: impl Data1D) -> ScatterLines {
        ScatterLines::create(self, zip_xy("scatterlines", x.to_vec_f64(), y.to_vec_f64()))
    }

    /// Makie's `scatterlines!(ax, points)` or `scatterlines!(ax, y)` (x = 1..=n).
    pub fn scatterlines_points(&self, p: impl PointData) -> ScatterLines {
        ScatterLines::create(self, p.to_points())
    }
}

impl crate::GridPosition {
    /// Makie's `scatterlines(fig[r, c], x, y)`: a new Axis at this position with the plot.
    #[track_caller]
    pub fn scatterlines(&self, x: impl Data1D, y: impl Data1D) -> ScatterLines {
        crate::Axis::new(self.clone()).scatterlines(x, y)
    }

    /// Makie's `scatterlines(fig[r, c], points)`: a new Axis at this position with the plot.
    pub fn scatterlines_points(&self, p: impl PointData) -> ScatterLines {
        crate::Axis::new(self.clone()).scatterlines_points(p)
    }
}

/// Makie's `scatterlines(x, y)`: a new Figure and Axis with lines and markers. Returns the plot
/// handle; call `.save(..)` or `.show()` on it.
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn scatterlines(x: impl Data1D, y: impl Data1D) -> ScatterLines {
    crate::Figure::new().at(1, 1).scatterlines(x, y)
}

/// Makie's `scatterlines(points)` / `scatterlines(y)`: a new Figure and Axis with the plot.
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn scatterlines_points(p: impl PointData) -> ScatterLines {
    crate::Figure::new().at(1, 1).scatterlines_points(p)
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::scene::drawlist::{ColorMapping, Prim, PrimColor};
    use crate::scene::{SceneCache, build};

    /// The `color = values` mappings of the line and of the markers.
    fn mappings(fig: &Figure) -> (Option<ColorMapping>, Option<ColorMapping>) {
        let (dl, _) = build(&fig.sh.snapshot(), None, &mut SceneCache::new());
        let (mut line, mut markers) = (None, None);
        for i in dl.items {
            match i.prim {
                Prim::Lines(l) => line = if let PrimColor::Values(_, m) = l.color { Some(m) } else { None },
                Prim::Markers(m) => markers = if let PrimColor::Values(_, m) = m.color { Some(m) } else { None },
                _ => {}
            }
        }
        (line, markers)
    }

    #[test]
    fn colormap_attributes() {
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        let s = ax.scatterlines([0.0, 1.0, 2.0], [0.0, 1.0, 0.0]).color(vec![0.0, 5.0, 10.0]);
        let (l, m) = mappings(&fig);
        let (l, m) = (l.expect("line colormapped"), m.expect("markers follow the line"));
        assert_eq!(l.lut.first(), Colormap::VIRIDIS.lut().first());
        assert_eq!(m.lut.first(), Colormap::VIRIDIS.lut().first());
        s.colormap(Colormap::MAGMA).colorrange((0, 20)).lowclip(RED).highclip(BLUE).nan_color(GREEN);
        let (l, _) = mappings(&fig);
        let l = l.unwrap();
        assert_eq!(l.lut.last(), Colormap::MAGMA.lut().last());
        // 0 -> -1, 10 -> 1, so 20 -> 3.
        assert!((l.range[0] + 1.0).abs() < 1e-6 && (l.range[1] - 3.0).abs() < 1e-6, "{:?}", l.range);
        assert_eq!((l.lowclip, l.highclip, l.nan_color), (Some(RED), Some(BLUE), GREEN));
        // Markers colored by their own values share the colormap attributes.
        s.color(BLACK).markercolor(vec![1.0, 2.0, 3.0]);
        let (l, m) = mappings(&fig);
        assert!(l.is_none());
        let m = m.expect("markers colormapped");
        assert_eq!(m.lut.last(), Colormap::MAGMA.lut().last());
        assert_eq!(m.highclip, Some(BLUE));
    }

    #[test]
    fn colorbar_follows_scatterlines() {
        let fig = Figure::new();
        let s = fig.at(1, 1).scatterlines([0.0, 1.0, 2.0], [0.0, 1.0, 0.0]).color(vec![2.0, 4.0, 3.0]);
        let cb = Colorbar::new(fig.at(1, 2), &s);
        let m = cb.colormapping().unwrap();
        assert!(m.mapped && m.colorrange == (2.0, 4.0) && m.colormap == Colormap::VIRIDIS);
        s.colormap(Colormap::MAGMA).colorrange((0, 10)).highclip(RED);
        let m = cb.colormapping().unwrap();
        assert_eq!((m.colormap, m.colorrange, m.highclip), (Colormap::MAGMA, (0.0, 10.0), Some(RED)));
        assert!(!s.color(BLACK).colormapping().unwrap().mapped);
    }
}
