//! `scatter`: markers at points.

use super::{ColorSpec, PlotImpl, PlotKind, add_to_axis, is_auto, plot_common, point_bounds, zip_xy};
use crate::attrs::attributes;
use crate::color::{Color, Colormap, MappingAttrs, encoded_values};
use crate::data::Data1D;
use crate::figure::{Dirty, FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::scene::drawlist::{Buf, BufKey, MarkersPrim, Prim, PrimColor};
use crate::style::Marker;
use std::sync::Arc;

/// A scatter plot handle (Makie's `Scatter`).
#[derive(Clone)]
pub struct Scatter {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct ScatterState {
    pub pos: Arc<Vec<[f64; 2]>>,
    pub attrs: ScatterAttrs,
}

attributes! {
    Scatter(ScatterAttrs, ScatterResolved, ScatterTheme) via with_attrs {
        /// A color, `Cycled(i)`, per-point colors, or values mapped through the colormap.
        color: ColorSpec = |_| ColorSpec::Auto, STYLE;
        marker: Marker = |_| Marker::Circle, STYLE;
        /// Marker size in units (Makie default 9; a `Circle` marker is 0.705 × this wide).
        markersize: f64 = |g| g.markersize, STYLE;
        strokecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        strokewidth: f64 = |_| 0.0, STYLE;
        /// Opacity multiplier.
        alpha: f64 = |_| 1.0, STYLE;
        /// Marker rotation in radians.
        rotation: f64 = |_| 0.0, STYLE;
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

plot_common!(Scatter);
super::color_mapped!(Scatter);

impl PlotImpl for ScatterState {
    fn cycle_group(&self) -> &'static str {
        "scatter"
    }

    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool {
        is_auto(self.attrs.color.as_ref(), theme.scatter.color.as_ref())
    }

    fn data_bounds(&self, xs: crate::transform::Scale, ys: crate::transform::Scale) -> Option<[f64; 4]> {
        point_bounds(&self.pos, xs, ys)
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.scatter, ctx.g);
        let alpha = r.alpha as f32;
        let pos = ctx.local_points(0, &self.pos);
        let color = match ctx.solid_color(&r.color, false) {
            Some(c) => PrimColor::Uniform(c.with_alpha(c.a * alpha)),
            None => match &r.color {
                ColorSpec::PerPoint(cs) => ctx.per_point(1, cs, alpha),
                ColorSpec::Values(v) => {
                    let e = encoded_values(v);
                    let map = MappingAttrs {
                        colormap: &r.colormap,
                        colorrange: r.colorrange,
                        lowclip: r.lowclip,
                        highclip: r.highclip,
                        nan_color: r.nan_color,
                        alpha: r.alpha,
                    }
                    .mapping(&e.enc);
                    let key = Some(BufKey { uid: ctx.uid, part: 2, rev: e.rev });
                    PrimColor::Values(Buf { key, data: e.data }, map)
                }
                _ => PrimColor::Uniform(ctx.g.palette[0]),
            },
        };
        ctx.push_data(Prim::Markers(MarkersPrim {
            pos,
            color,
            size: r.markersize as f32,
            sizes: None,
            marker: r.marker,
            stroke_color: r.strokecolor.with_alpha(r.strokecolor.a * alpha),
            stroke_width: r.strokewidth as f32,
            rotation: r.rotation as f32,
        }));
    }

    fn colormapping(&self, theme: &crate::theme::Theme, g: &crate::theme::Globals) -> Option<super::ResolvedColormap> {
        let r = self.attrs.resolve(&theme.scatter, g);
        let ColorSpec::Values(v) = &r.color else {
            return Some(super::ResolvedColormap::unmapped(r.colormap, r.alpha));
        };
        let [lo, hi] = r.colorrange.unwrap_or_else(|| encoded_values(v).enc.auto_range());
        Some(super::ResolvedColormap {
            colormap: r.colormap,
            colorrange: (lo, hi),
            lowclip: r.lowclip,
            highclip: r.highclip,
            alpha: r.alpha,
            mapped: true,
        })
    }

    fn pick(&self, ctx: &mut super::pick::PickCtx<'_>) -> Option<super::pick::Hover> {
        let (i, dist, anchor) = ctx.nearest_point(0, &self.pos)?;
        let r = self.attrs.resolve(&ctx.theme.scatter, ctx.g);
        let [x, y] = self.pos[i];
        Some(super::pick::Hover {
            dist,
            anchor,
            text: super::pick::point_text(x, y),
            ring: Some(r.markersize + 2.0 * r.strokewidth + 4.0),
        })
    }
}

impl Scatter {
    fn with_attrs(&self, f: impl FnOnce(&mut ScatterAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Scatter(s) = &mut p.kind {
                f(&mut s.attrs)
            }
        });
    }

    pub(crate) fn create(ax: &crate::Axis, pos: Vec<[f64; 2]>) -> Scatter {
        let st = ScatterState { pos: Arc::new(pos), attrs: ScatterAttrs::default() };
        let id = add_to_axis(ax, PlotKind::Scatter(st));
        Scatter { sh: ax.sh.clone(), id }
    }

    /// Replaces the points (the lengths of x and y must match).
    #[track_caller]
    pub fn set_data(&self, x: impl Data1D, y: impl Data1D) -> Scatter {
        let pos = Arc::new(zip_xy("Scatter::set_data", x.to_vec_f64(), y.to_vec_f64()));
        self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            if let PlotKind::Scatter(s) = &mut p.kind {
                s.pos = pos;
                p.data_rev += 1;
            }
        });
        self.clone()
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Scatter(s)) => s.pos.len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl crate::Axis {
    /// Makie's `scatter!(ax, x, y)`.
    #[track_caller]
    pub fn scatter(&self, x: impl Data1D, y: impl Data1D) -> Scatter {
        Scatter::create(self, zip_xy("scatter", x.to_vec_f64(), y.to_vec_f64()))
    }
}

impl crate::GridPosition {
    /// Makie's `scatter(fig[r, c], x, y)`: a new Axis at this position with a scatter plot.
    #[track_caller]
    pub fn scatter(&self, x: impl Data1D, y: impl Data1D) -> Scatter {
        crate::Axis::new(self.clone()).scatter(x, y)
    }
}

/// Makie's `scatter(x, y)`: a new Figure and Axis with a scatter plot. Returns the plot handle;
/// call `.save(..)` or `.show()` on it.
///
/// ```no_run
/// let x: Vec<f64> = (0..100).map(|i| i as f64).collect();
/// ezviz::scatter(&x, x.iter().map(|v| v.sin())).save("scatter.png").unwrap();
/// ```
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn scatter(x: impl Data1D, y: impl Data1D) -> Scatter {
    crate::Figure::new().at(1, 1).scatter(x, y)
}
