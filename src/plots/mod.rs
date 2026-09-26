//! Plot types. Each plot lives in an Axis; its handle is `Clone + Send + Sync` and updates the
//! plot in place.

pub(crate) mod band;
pub(crate) mod barplot;
pub(crate) mod bars;
pub(crate) mod heatmap;
pub(crate) mod hist;
pub(crate) mod legend_elements;
pub(crate) mod pick;
pub(crate) mod reflines;
pub(crate) mod scatter;
pub(crate) mod textplot;

pub use band::Band;
pub use barplot::{BarPlot, BarX};
pub use heatmap::Heatmap;
pub use hist::{Bins, Hist};
pub use legend_elements::LegendElement;
pub use reflines::{ABLines, HLines, RefLines, RefValues, VLines};
pub use scatter::Scatter;
pub use textplot::{IntoTexts, TextPlot};
pub(crate) mod lines;
pub(crate) mod scatterlines;

pub use lines::Lines;
pub use scatterlines::ScatterLines;

use crate::attrs::Conv;
use crate::color::{Color, Colormap, IntoColor};
use crate::data::Scalar;
use crate::figure::BlockId;
use crate::text::RichText;
use std::sync::Arc;

/// A plot's color: one color, a palette entry, per-point colors, or values mapped through the
/// colormap.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum ColorSpec {
    /// Cycle through the theme palette (Makie's default).
    #[default]
    Auto,
    Solid(Color),
    /// 1-based palette index (Makie's `Cycled(i)`).
    Cycled(usize),
    /// One value per point, mapped through the plot's colormap.
    Values(Arc<Vec<f64>>),
    /// One color per point.
    PerPoint(Arc<Vec<Color>>),
}

/// Makie's `Cycled(i)`: the i-th (1-based) palette color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cycled(pub usize);

impl<C: IntoColor> Conv<ColorSpec> for C {
    #[track_caller]
    fn conv(self) -> ColorSpec {
        ColorSpec::Solid(self.into_color())
    }
}
impl Conv<ColorSpec> for Cycled {
    fn conv(self) -> ColorSpec {
        ColorSpec::Cycled(self.0)
    }
}
impl Conv<ColorSpec> for ColorSpec {
    fn conv(self) -> ColorSpec {
        self
    }
}
impl<T: Scalar> Conv<ColorSpec> for &[T] {
    fn conv(self) -> ColorSpec {
        ColorSpec::Values(Arc::new(self.iter().map(|v| v.to_f64()).collect()))
    }
}
impl<T: Scalar> Conv<ColorSpec> for &Vec<T> {
    fn conv(self) -> ColorSpec {
        ColorSpec::Values(Arc::new(self.iter().map(|v| v.to_f64()).collect()))
    }
}
impl<T: Scalar> Conv<ColorSpec> for Vec<T> {
    fn conv(self) -> ColorSpec {
        ColorSpec::Values(Arc::new(self.into_iter().map(|v| v.to_f64()).collect()))
    }
}
impl Conv<ColorSpec> for &[Color] {
    fn conv(self) -> ColorSpec {
        ColorSpec::PerPoint(Arc::new(self.to_vec()))
    }
}
impl Conv<ColorSpec> for Vec<Color> {
    fn conv(self) -> ColorSpec {
        ColorSpec::PerPoint(Arc::new(self))
    }
}
/// Attributes every plot has.
#[derive(Clone, Debug)]
pub(crate) struct PlotCommon {
    pub label: Option<RichText>,
    pub visible: bool,
    pub z: f32,
    pub inspectable: bool,
    pub xautolimits: bool,
    pub yautolimits: bool,
}

impl Default for PlotCommon {
    fn default() -> Self {
        PlotCommon { label: None, visible: true, z: 0.0, inspectable: true, xautolimits: true, yautolimits: true }
    }
}

/// What every plot type implements. Adding a plot type = a module implementing this trait plus
/// one line in `plot_kinds!` below.
pub(crate) trait PlotImpl {
    /// Name of the per-axis cycle counter this plot advances (Makie counts per plot function).
    fn cycle_group(&self) -> &'static str;
    /// Whether the cycled attribute (color) is automatic: set neither on the plot nor in the theme.
    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool;
    /// Finite data bounds in scaled space `[x0, x1, y0, y1]`.
    fn data_bounds(&self, xs: crate::transform::Scale, ys: crate::transform::Scale) -> Option<[f64; 4]>;
    /// Plot types that make the axis use tight limits (heatmaps).
    fn tight_limits(&self) -> bool {
        false
    }
    /// Category names for tick labels, and whether they are on the x axis (categorical barplots).
    fn categories(&self) -> Option<(bool, Arc<Vec<String>>)> {
        None
    }
    /// Lowers the plot to draw-list primitives.
    fn emit(&self, ctx: &mut crate::scene::PlotCtx<'_>);
    /// Hover inspection: the element nearest to the cursor within `ctx.radius`, if any.
    fn pick(&self, _ctx: &mut pick::PickCtx<'_>) -> Option<pick::Hover> {
        None
    }
    /// How the plot looks in a legend entry (Makie's `legendelements`; none by default).
    fn legend_elements(&self, _ctx: &legend_elements::LegendCtx<'_>) -> Vec<legend_elements::LegendElement> {
        Vec::new()
    }

    /// The plot's resolved color mapping, for plot types that can map values through a colormap
    /// (`None` for plot types without a colormap).
    fn colormapping(&self, _theme: &crate::theme::Theme, _g: &crate::theme::Globals) -> Option<ResolvedColormap> {
        None
    }
}

/// A plot's color mapping as it is drawn right now (Makie's `ColorMapping`): what a
/// [`Colorbar`](crate::Colorbar) linked to the plot shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedColormap {
    pub colormap: Colormap,
    /// The colorrange in effect: the explicit one, or the finite extrema of the values.
    pub colorrange: (f64, f64),
    /// Explicit color for values below the range (`None`: the first colormap color, no triangle).
    pub lowclip: Option<Color>,
    /// Explicit color for values above the range (`None`: the last colormap color, no triangle).
    pub highclip: Option<Color>,
    /// Opacity multiplier applied to the colormap.
    pub alpha: f64,
    /// `false` when the plot's colors are not values mapped through the colormap (a solid or
    /// per-point color); `colorrange` is then `(0, 1)`.
    pub mapped: bool,
}

impl ResolvedColormap {
    /// The mapping of a plot that is not colored by values: its colormap over `(0, 1)`.
    pub(crate) fn unmapped(colormap: Colormap, alpha: f64) -> ResolvedColormap {
        ResolvedColormap { colormap, colorrange: (0.0, 1.0), lowclip: None, highclip: None, alpha, mapped: false }
    }
}

/// Plot handles whose colors can come from a colormap (heatmaps; scatter and lines colored by
/// values). A [`Colorbar`](crate::Colorbar) created from one follows its colormap, colorrange,
/// clip colors and alpha on every frame.
pub trait ColorMapped {
    #[doc(hidden)]
    fn plot_ref(&self) -> PlotRef;

    /// The plot's current color mapping (`None` if the plot was deleted).
    fn colormapping(&self) -> Option<ResolvedColormap> {
        let r = self.plot_ref();
        let st = r.sh.state.lock();
        let g = st.theme.globals();
        st.plot(r.id)?.kind.imp().colormapping(&st.theme, &g)
    }
}

/// An opaque reference to a plot (see [`ColorMapped`]).
#[doc(hidden)]
#[derive(Clone)]
pub struct PlotRef {
    pub(crate) sh: Arc<crate::figure::FigShared>,
    pub(crate) id: crate::figure::PlotId,
}

/// Implements [`ColorMapped`] for a plot handle.
macro_rules! color_mapped {
    ($Handle:ident) => {
        impl $crate::plots::ColorMapped for $Handle {
            fn plot_ref(&self) -> $crate::plots::PlotRef {
                $crate::plots::PlotRef { sh: self.sh.clone(), id: self.id }
            }
        }
    };
}
pub(crate) use color_mapped;

/// Declares the plot-type registry: `PlotKind` and dispatch to each type's `PlotImpl`.
macro_rules! plot_kinds {
    ($($V:ident($T:ty)),* $(,)?) => {
        #[derive(Clone, Debug)]
        pub(crate) enum PlotKind { $($V($T)),* }

        impl PlotKind {
            pub(crate) fn imp(&self) -> &dyn PlotImpl {
                match self { $(PlotKind::$V(s) => s),* }
            }
        }
    };
}

plot_kinds! {
    Scatter(scatter::ScatterState),
    BarPlot(barplot::BarPlotState),
    Hist(hist::HistState),
    Band(band::BandState),
    Text(textplot::TextState),
    Heatmap(heatmap::HeatmapState),
    Lines(lines::LinesState),
    ScatterLines(scatterlines::ScatterLinesState),
    RefLines(reflines::RefLinesState),
}

/// `true` if a color spec (explicit or themed) is automatic.
pub(crate) fn is_auto(explicit: Option<&ColorSpec>, theme: Option<&ColorSpec>) -> bool {
    matches!(explicit.or(theme), None | Some(ColorSpec::Auto))
}

/// Finite bounds of points after applying the scales.
pub(crate) fn point_bounds(
    pts: &[[f64; 2]],
    xs: crate::transform::Scale,
    ys: crate::transform::Scale,
) -> Option<[f64; 4]> {
    let mut b = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
    for p in pts {
        let (x, y) = (xs.forward(p[0]), ys.forward(p[1]));
        if x.is_finite() && y.is_finite() {
            b[0] = b[0].min(x);
            b[1] = b[1].max(x);
            b[2] = b[2].min(y);
            b[3] = b[3].max(y);
        }
    }
    (b[0] <= b[1]).then_some(b)
}

#[derive(Clone, Debug)]
pub(crate) struct PlotSlot {
    pub generation: u32,
    pub uid: u64,
    pub axis: BlockId,
    pub common: PlotCommon,
    pub kind: PlotKind,
    /// Bumped on every data change (drives GPU re-uploads).
    pub data_rev: u64,
}

impl PlotSlot {
    pub(crate) fn new(axis: BlockId, kind: PlotKind) -> PlotSlot {
        PlotSlot {
            generation: 0,
            uid: crate::figure::next_uid(),
            axis,
            common: PlotCommon::default(),
            kind,
            data_rev: 0,
        }
    }
}

/// Converts x/y data into point pairs, panicking with a helpful message on length mismatch.
#[track_caller]
pub(crate) fn zip_xy(what: &str, x: Vec<f64>, y: Vec<f64>) -> Vec<[f64; 2]> {
    assert!(x.len() == y.len(), "{what}: x has {} values but y has {}", x.len(), y.len());
    x.into_iter().zip(y).map(|(a, b)| [a, b]).collect()
}

/// Generates the methods every plot handle shares.
macro_rules! plot_common {
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
            /// Draw order within the axis (higher is on top; default 0).
            pub fn z(&self, z: f32) -> Self {
                self.with_slot($crate::figure::Dirty::STYLE, |p| p.common.z = z);
                self.clone()
            }
            /// Whether the hover inspector reports this plot.
            pub fn inspectable(&self, v: bool) -> Self {
                self.with_slot($crate::figure::Dirty::STYLE, |p| p.common.inspectable = v);
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
            /// The axis containing this plot.
            pub fn axis(&self) -> $crate::Axis {
                let id = self.sh.state.lock().plot(self.id).map(|p| p.axis).expect("plot was deleted");
                $crate::Axis { sh: self.sh.clone(), id }
            }
            /// Makie's `fig, ax, plt = scatter(...)`.
            pub fn unpack(&self) -> ($crate::Figure, $crate::Axis, Self) {
                (self.figure(), self.axis(), self.clone())
            }
            /// Runs `f` on this plot's axis and returns the plot (for one-liners).
            pub fn with_axis<R>(&self, f: impl FnOnce($crate::Axis) -> R) -> Self {
                f(self.axis());
                self.clone()
            }
            /// Runs `f` on this plot's figure and returns the plot (for one-liners).
            pub fn with_figure<R>(&self, f: impl FnOnce($crate::Figure) -> R) -> Self {
                f(self.figure());
                self.clone()
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
                        if let Some(a) = st.block_mut(axis).and_then(|b| b.as_axis_mut()) {
                            a.plots.retain(|p| *p != id);
                        }
                        st.plots[id.index as usize] = None;
                    }
                });
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
pub(crate) use plot_common;

/// Adds a plot to an axis under the lock; returns its id.
pub(crate) fn add_to_axis(ax: &crate::Axis, kind: PlotKind) -> crate::figure::PlotId {
    let axis_id = ax.id;
    ax.sh.update(crate::figure::Dirty::LAYOUT, |st| {
        let id = st.add_plot(PlotSlot::new(axis_id, kind));
        match st.block_mut(axis_id).and_then(|b| b.as_axis_mut()) {
            Some(a) => a.plots.push(id),
            None => panic!("plotting into an Axis that no longer exists"),
        }
        id
    })
}
