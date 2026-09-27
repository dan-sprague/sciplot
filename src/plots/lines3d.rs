//! `lines` in an [`Axis3`](crate::Axis3): a 3D polyline (NaN breaks it), e.g. a trajectory.
//!
//! Drawn as screen-space segments with round ends and joins, `linewidth` in units, hidden behind
//! surfaces by the depth buffer (lines don't occlude each other: later ones draw on top, as in
//! CairoMakie). Live data: [`Lines3d::push`] appends in O(1) and uploads only the new points.

use super::{ColorSpec, PlotImpl, PlotKind, is_auto};
use crate::attrs::attributes;
use crate::color::{Color, Colormap, MappingAttrs, encoded_values};
use crate::data::{Data1D, Scalar};
use crate::figure::{FigShared, PlotId};
use crate::scene::axis3::{Plot3dCtx, Plot3dImpl, Points3};
use crate::scene::drawlist::{Buf, BufKey, Lines3dPrim, Prim, PrimColor};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// A 3D line plot handle (Makie's `lines!(ax3, x, y, z)`).
///
/// ```no_run
/// use sciplot::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis3::new(fig.at(1, 1));
/// let t = linspace(0.0, 30.0, 1000);
/// ax.lines(t.iter().map(|t| t.cos()), t.iter().map(|t| t.sin()), &t).color(&t);
/// fig.save("helix.png").unwrap();
/// ```
#[derive(Clone)]
pub struct Lines3d {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct Lines3dState {
    pub pts: Points3,
    pub attrs: Lines3dAttrs,
    pub style_rev: u64,
}

attributes! {
    Lines3d(Lines3dAttrs, Lines3dResolved, Lines3dTheme) via with_attrs {
        /// A color, `Cycled(i)`, per-point colors, or per-point values mapped through the colormap
        /// (interpolated along each segment).
        color: ColorSpec = |_| ColorSpec::Auto, STYLE;
        /// Line width in units (Makie default 1.5).
        linewidth: f64 = |g| g.linewidth, STYLE;
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

/// Generates the methods every 3D plot handle shares.
macro_rules! plot3d_common {
    ($Handle:ident) => {
        impl $Handle {
            pub(crate) fn with_slot<R>(
                &self,
                dirty: u8,
                f: impl FnOnce(&mut $crate::plots::PlotSlot) -> R,
            ) -> Option<R> {
                let r = self.sh.update(dirty, |st| st.plot_mut(self.id).map(f));
                if r.is_none() {
                    $crate::warn_once(concat!("setter called on a ", stringify!($Handle), " that no longer exists"));
                }
                r
            }
            /// Legend label.
            pub fn label(&self, s: impl Into<$crate::text::RichText>) -> Self {
                let s = s.into();
                self.with_slot($crate::figure::Dirty::LAYOUT, |p| p.common.label = Some(s));
                self.clone()
            }
            /// Show or hide the plot.
            pub fn visible(&self, v: bool) -> Self {
                self.with_slot($crate::figure::Dirty::STYLE, |p| p.common.visible = v);
                self.clone()
            }
            /// Whether this plot's x data counts toward automatic limits.
            pub fn xautolimits(&self, v: bool) -> Self {
                self.with_slot($crate::figure::Dirty::LIMITS, |p| p.common.xautolimits = v);
                self.clone()
            }
            /// Whether this plot's y data counts toward automatic limits.
            pub fn yautolimits(&self, v: bool) -> Self {
                self.with_slot($crate::figure::Dirty::LIMITS, |p| p.common.yautolimits = v);
                self.clone()
            }
            /// The figure containing this plot.
            pub fn figure(&self) -> $crate::Figure {
                $crate::Figure { sh: self.sh.clone() }
            }
            /// The Axis3 containing this plot.
            pub fn axis(&self) -> $crate::Axis3 {
                let id = self.sh.state.lock().plot(self.id).map(|p| p.axis).expect("plot was deleted");
                $crate::Axis3 { sh: self.sh.clone(), id }
            }
            /// Makie's `fig, ax, plt = lines(...)`.
            pub fn unpack(&self) -> ($crate::Figure, $crate::Axis3, Self) {
                (self.figure(), self.axis(), self.clone())
            }
            /// Saves the whole figure (`.png` or `.svg`).
            pub fn save(&self, path: impl AsRef<std::path::Path>) -> $crate::Result<()> {
                self.figure().save(path)
            }
            /// Saves the whole figure with options.
            pub fn save_with(&self, path: impl AsRef<std::path::Path>, opts: $crate::Save) -> $crate::Result<()> {
                self.figure().save_with(path, opts)
            }
            /// Opens the figure in a window and blocks until it is closed.
            #[cfg(feature = "window")]
            pub fn show(&self) -> $crate::Result<()> {
                self.figure().show()
            }
            /// Removes the plot from its axis.
            pub fn delete(&self) {
                let id = self.id;
                self.sh.update($crate::figure::Dirty::LAYOUT, |st| {
                    let axis = st.plot(id).map(|p| p.axis);
                    if let Some(axis) = axis {
                        if let Some($crate::blocks::Block::Axis3(a)) = st.block_mut(axis) {
                            a.plots.retain(|p| *p != id);
                        }
                        st.plots[id.index as usize] = None;
                    }
                });
            }
        }

        impl From<&$Handle> for $crate::plots::PlotRef {
            fn from(p: &$Handle) -> $crate::plots::PlotRef {
                $crate::plots::PlotRef { sh: p.sh.clone(), id: p.id }
            }
        }

        impl From<$Handle> for $crate::plots::PlotRef {
            fn from(p: $Handle) -> $crate::plots::PlotRef {
                $crate::plots::PlotRef { sh: p.sh, id: p.id }
            }
        }

        impl PartialEq for $Handle {
            fn eq(&self, other: &Self) -> bool {
                std::sync::Arc::ptr_eq(&self.sh, &other.sh) && self.id == other.id
            }
        }

        impl std::fmt::Debug for $Handle {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!(stringify!($Handle), "#{}"), self.id.index)
            }
        }
    };
}
pub(crate) use plot3d_common;

plot3d_common!(Lines3d);
super::color_mapped!(Lines3d);

/// Adds a plot to an Axis3 under the lock; returns its id.
pub(crate) fn add_to_axis3(ax: &crate::Axis3, kind: PlotKind) -> PlotId {
    let axis_id = ax.id;
    ax.sh.update(crate::figure::Dirty::LAYOUT, |st| {
        let id = st.add_plot(super::PlotSlot::new(axis_id, kind));
        match st.block_mut(axis_id) {
            Some(crate::blocks::Block::Axis3(a)) => a.plots.push(id),
            _ => panic!("plotting into an Axis3 that no longer exists"),
        }
        id
    })
}

/// Zips x, y and z into points, panicking with a helpful message on a length mismatch.
#[track_caller]
pub(crate) fn zip_xyz(what: &str, x: Vec<f64>, y: Vec<f64>, z: Vec<f64>) -> Vec<[f64; 3]> {
    assert!(
        x.len() == y.len() && y.len() == z.len(),
        "{what}: x, y and z have {}, {} and {} values",
        x.len(),
        y.len(),
        z.len()
    );
    x.into_iter().zip(y).zip(z).map(|((a, b), c)| [a, b, c]).collect()
}

/// Lowers a color spec for `n` elements of a 3D plot: uniform, premultiplied per-element colors
/// (cached under `part`), or values through the colormap (`map`).
pub(crate) fn prim_color3(
    ctx: &Plot3dCtx<'_>,
    spec: &ColorSpec,
    alpha: f64,
    n: usize,
    part: u8,
    style_rev: u64,
    map: &MappingAttrs<'_>,
) -> PrimColor {
    let a = alpha as f32;
    if let Some(c) = ctx.solid_color(spec) {
        return PrimColor::Uniform(c.with_alpha(c.a * a));
    }
    match spec {
        ColorSpec::PerPoint(cs) if cs.len() == n => {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (ctx.data_rev, style_rev, part).hash(&mut h);
            let data = cs.iter().map(|c| c.with_alpha(c.a * a).to_premul_u32()).collect();
            PrimColor::PerElement(Buf {
                key: Some(BufKey { uid: ctx.uid, part, rev: h.finish() }),
                data: Arc::new(data),
            })
        }
        ColorSpec::Values(v) if v.len() == n => {
            let e = encoded_values(v);
            let mapping = MappingAttrs { alpha, ..*map }.mapping(&e.enc);
            PrimColor::Values(Buf { key: Some(BufKey { uid: ctx.uid, part, rev: e.rev }), data: e.data }, mapping)
        }
        _ => {
            crate::warn_once("per-point colors of a 3D plot must have one entry per point; using the palette color");
            let c = ctx.g.palette[ctx.cycle % ctx.g.palette.len().max(1)];
            PrimColor::Uniform(c.with_alpha(c.a * a))
        }
    }
}

/// The resolved colormap of a plot colored by `spec` (see `PlotImpl::colormapping`).
pub(crate) fn colormapping3(spec: &ColorSpec, map: &MappingAttrs<'_>) -> super::ResolvedColormap {
    let ColorSpec::Values(v) = spec else {
        return super::ResolvedColormap::unmapped(map.colormap.clone(), map.alpha);
    };
    let [lo, hi] = map.colorrange.unwrap_or_else(|| encoded_values(v).enc.auto_range());
    super::ResolvedColormap {
        colormap: map.colormap.clone(),
        colorrange: (lo, hi),
        lowclip: map.lowclip,
        highclip: map.highclip,
        alpha: map.alpha,
        mapped: true,
    }
}

impl Lines3dResolved {
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

impl PlotImpl for Lines3dState {
    fn cycle_group(&self) -> &'static str {
        "lines"
    }

    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool {
        is_auto(self.attrs.color.as_ref(), theme.lines3d.color.as_ref())
    }

    /// Not a 2D plot: it lives in an Axis3.
    fn data_bounds(&self, _: crate::transform::Scale, _: crate::transform::Scale) -> Option<[f64; 4]> {
        None
    }

    fn emit(&self, _ctx: &mut crate::scene::PlotCtx<'_>) {}

    fn legend_elements(&self, ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        let r = self.attrs.resolve(&ctx.theme.lines3d, ctx.g);
        vec![super::LegendElement::Line {
            color: ctx.color(&r.color, false, r.alpha, super::legend_elements::DEFAULT_LINECOLOR),
            linewidth: r.linewidth,
            linestyle: crate::style::Linestyle::Solid,
        }]
    }

    fn colormapping(&self, theme: &crate::theme::Theme, g: &crate::theme::Globals) -> Option<super::ResolvedColormap> {
        let r = self.attrs.resolve(&theme.lines3d, g);
        Some(colormapping3(&r.color, &r.mapping()))
    }
}

impl Plot3dImpl for Lines3dState {
    fn bounds(&self) -> Option<[f64; 6]> {
        self.pts.bounds()
    }

    fn emit3(&self, ctx: &mut Plot3dCtx<'_>) {
        if self.pts.len() < 2 {
            return;
        }
        let r = self.attrs.resolve(&ctx.theme.lines3d, ctx.g);
        let pts = ctx.local_points_append(0, &self.pts);
        let color = prim_color3(ctx, &r.color, r.alpha, self.pts.len(), 1, self.style_rev, &r.mapping());
        let view = ctx.view.clone();
        ctx.push(Prim::Lines3d(Lines3dPrim { view, pts, color, width: r.linewidth as f32, append: true }));
    }
}

impl Lines3d {
    fn with_attrs(&self, f: impl FnOnce(&mut Lines3dAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Lines3d(s) = &mut p.kind {
                f(&mut s.attrs);
                s.style_rev += 1;
            }
        });
    }

    fn with_points(&self, f: impl FnOnce(&mut Points3)) {
        self.with_slot(crate::figure::Dirty::DATA | crate::figure::Dirty::LIMITS, |p| {
            if let PlotKind::Lines3d(s) = &mut p.kind {
                f(&mut s.pts);
                p.data_rev += 1;
            }
        });
    }

    /// Replaces the points (x, y and z must have the same length).
    #[track_caller]
    pub fn set_data(&self, x: impl Data1D, y: impl Data1D, z: impl Data1D) -> Self {
        let pts = Points3::new(zip_xyz("Lines3d::set_data", x.to_vec_f64(), y.to_vec_f64(), z.to_vec_f64()));
        self.with_points(move |p| *p = pts);
        self.clone()
    }

    /// Appends one point: O(1) amortized, and only new points are uploaded to the GPU.
    pub fn push(&self, x: impl Scalar, y: impl Scalar, z: impl Scalar) -> Self {
        let q = [x.to_f64(), y.to_f64(), z.to_f64()];
        self.with_points(move |p| p.push(q));
        self.clone()
    }

    /// Appends points (x, y and z must have the same length).
    #[track_caller]
    pub fn extend(&self, x: impl Data1D, y: impl Data1D, z: impl Data1D) -> Self {
        let v = zip_xyz("Lines3d::extend", x.to_vec_f64(), y.to_vec_f64(), z.to_vec_f64());
        self.with_points(move |p| p.extend_from_slice(&v));
        self.clone()
    }

    /// Removes every point.
    pub fn clear(&self) -> Self {
        self.with_points(|p| *p = Points3::default());
        self.clone()
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Lines3d(s)) => s.pts.len(),
            _ => 0,
        }
    }

    /// Whether there are no points.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl crate::Axis3 {
    /// Makie's `lines!(ax3, x, y, z)`.
    #[track_caller]
    pub fn lines(&self, x: impl Data1D, y: impl Data1D, z: impl Data1D) -> Lines3d {
        let pts = Points3::new(zip_xyz("lines", x.to_vec_f64(), y.to_vec_f64(), z.to_vec_f64()));
        let st = Lines3dState { pts, attrs: Lines3dAttrs::default(), style_rev: 0 };
        let id = add_to_axis3(self, PlotKind::Lines3d(st));
        Lines3d { sh: self.sh.clone(), id }
    }
}

impl crate::GridPosition {
    /// Makie's `lines(fig[r, c], x, y, z)`: a new Axis3 at this position with a 3D line.
    #[track_caller]
    pub fn lines3d(&self, x: impl Data1D, y: impl Data1D, z: impl Data1D) -> Lines3d {
        crate::Axis3::new(self.clone()).lines(x, y, z)
    }
}

/// Makie's `lines(x, y, z)`: a new Figure and Axis3 with a 3D line.
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn lines3d(x: impl Data1D, y: impl Data1D, z: impl Data1D) -> Lines3d {
    crate::Figure::new().at(1, 1).lines3d(x, y, z)
}
