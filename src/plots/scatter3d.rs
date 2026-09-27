//! `scatter` in an [`Axis3`](crate::Axis3): screen-space markers (`markersize` in units, Makie's
//! `markerspace = :pixel`) at 3D positions, hidden behind surfaces by the depth buffer.

use super::lines3d::{add_to_axis3, colormapping3, plot3d_common, prim_color3, zip_xyz};
use super::{ColorSpec, PlotImpl, PlotKind, is_auto};
use crate::attrs::attributes;
use crate::color::{Color, Colormap, MappingAttrs};
use crate::data::{Data1D, Scalar};
use crate::figure::{FigShared, PlotId};
use crate::scene::axis3::{Plot3dCtx, Plot3dImpl, Points3};
use crate::scene::drawlist::{Markers3dPrim, Prim};
use crate::style::Marker;
use std::sync::Arc;

/// A 3D scatter plot handle (Makie's `scatter!(ax3, x, y, z)`).
///
/// ```no_run
/// use ezviz::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis3::new(fig.at(1, 1));
/// ax.scatter([0.0, 1.0, 2.0], [1.0, 0.0, 1.0], [0.0, 0.5, 1.0]).markersize(12);
/// fig.save("cloud.png").unwrap();
/// ```
#[derive(Clone)]
pub struct Scatter3d {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct Scatter3dState {
    pub pts: Points3,
    pub attrs: Scatter3dAttrs,
    pub style_rev: u64,
}

attributes! {
    Scatter3d(Scatter3dAttrs, Scatter3dResolved, Scatter3dTheme) via with_attrs {
        /// A color, `Cycled(i)`, per-point colors, or values mapped through the colormap.
        color: ColorSpec = |_| ColorSpec::Auto, STYLE;
        marker: Marker = |_| Marker::Circle, STYLE;
        /// Marker size in units (Makie default 9).
        markersize: f64 = |g| g.markersize, STYLE;
        strokecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        strokewidth: f64 = |_| 0.0, STYLE;
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

plot3d_common!(Scatter3d);
super::color_mapped!(Scatter3d);

impl Scatter3dResolved {
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

impl PlotImpl for Scatter3dState {
    fn cycle_group(&self) -> &'static str {
        "scatter"
    }

    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool {
        is_auto(self.attrs.color.as_ref(), theme.scatter3d.color.as_ref())
    }

    /// Not a 2D plot: it lives in an Axis3.
    fn data_bounds(&self, _: crate::transform::Scale, _: crate::transform::Scale) -> Option<[f64; 4]> {
        None
    }

    fn emit(&self, _ctx: &mut crate::scene::PlotCtx<'_>) {}

    fn legend_elements(&self, ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        let r = self.attrs.resolve(&ctx.theme.scatter3d, ctx.g);
        vec![super::LegendElement::Marker {
            color: ctx.color(&r.color, false, r.alpha, super::legend_elements::DEFAULT_MARKERCOLOR),
            marker: r.marker,
            markersize: r.markersize,
            strokecolor: r.strokecolor.with_alpha(r.strokecolor.a * r.alpha as f32),
            strokewidth: r.strokewidth,
        }]
    }

    fn colormapping(&self, theme: &crate::theme::Theme, g: &crate::theme::Globals) -> Option<super::ResolvedColormap> {
        let r = self.attrs.resolve(&theme.scatter3d, g);
        Some(colormapping3(&r.color, &r.mapping()))
    }
}

impl Plot3dImpl for Scatter3dState {
    fn bounds(&self) -> Option<[f64; 6]> {
        self.pts.bounds()
    }

    fn emit3(&self, ctx: &mut Plot3dCtx<'_>) {
        if self.pts.is_empty() {
            return;
        }
        let r = self.attrs.resolve(&ctx.theme.scatter3d, ctx.g);
        let pos = ctx.local_points_append(0, &self.pts);
        let color = prim_color3(ctx, &r.color, r.alpha, self.pts.len(), 1, self.style_rev, &r.mapping());
        let view = ctx.view.clone();
        let a = r.alpha as f32;
        ctx.push(Prim::Markers3d(Markers3dPrim {
            view,
            pos,
            color,
            size: r.markersize as f32,
            sizes: None,
            marker: r.marker,
            stroke_color: r.strokecolor.with_alpha(r.strokecolor.a * a),
            stroke_width: r.strokewidth as f32,
            append: true,
        }));
    }
}

impl Scatter3d {
    fn with_attrs(&self, f: impl FnOnce(&mut Scatter3dAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Scatter3d(s) = &mut p.kind {
                f(&mut s.attrs);
                s.style_rev += 1;
            }
        });
    }

    fn with_points(&self, f: impl FnOnce(&mut Points3)) {
        self.with_slot(crate::figure::Dirty::DATA | crate::figure::Dirty::LIMITS, |p| {
            if let PlotKind::Scatter3d(s) = &mut p.kind {
                f(&mut s.pts);
                p.data_rev += 1;
            }
        });
    }

    /// Replaces the points (x, y and z must have the same length).
    #[track_caller]
    pub fn set_data(&self, x: impl Data1D, y: impl Data1D, z: impl Data1D) -> Self {
        let pts = Points3::new(zip_xyz("Scatter3d::set_data", x.to_vec_f64(), y.to_vec_f64(), z.to_vec_f64()));
        self.with_points(move |p| *p = pts);
        self.clone()
    }

    /// Appends one point (O(1) amortized).
    pub fn push(&self, x: impl Scalar, y: impl Scalar, z: impl Scalar) -> Self {
        let q = [x.to_f64(), y.to_f64(), z.to_f64()];
        self.with_points(move |p| p.push(q));
        self.clone()
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Scatter3d(s)) => s.pts.len(),
            _ => 0,
        }
    }

    /// Whether there are no points.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl crate::Axis3 {
    /// Makie's `scatter!(ax3, x, y, z)`.
    #[track_caller]
    pub fn scatter(&self, x: impl Data1D, y: impl Data1D, z: impl Data1D) -> Scatter3d {
        let pts = Points3::new(zip_xyz("scatter", x.to_vec_f64(), y.to_vec_f64(), z.to_vec_f64()));
        let st = Scatter3dState { pts, attrs: Scatter3dAttrs::default(), style_rev: 0 };
        let id = add_to_axis3(self, PlotKind::Scatter3d(st));
        Scatter3d { sh: self.sh.clone(), id }
    }
}

impl crate::GridPosition {
    /// Makie's `scatter(fig[r, c], x, y, z)`: a new Axis3 at this position with a 3D scatter.
    #[track_caller]
    pub fn scatter3d(&self, x: impl Data1D, y: impl Data1D, z: impl Data1D) -> Scatter3d {
        crate::Axis3::new(self.clone()).scatter(x, y, z)
    }
}

/// Makie's `scatter(x, y, z)`: a new Figure and Axis3 with a 3D scatter.
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn scatter3d(x: impl Data1D, y: impl Data1D, z: impl Data1D) -> Scatter3d {
    crate::Figure::new().at(1, 1).scatter3d(x, y, z)
}
