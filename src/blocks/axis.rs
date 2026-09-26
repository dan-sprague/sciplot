//! `Axis`: a 2D coordinate system with ticks, labels, grid and spines.

use crate::attrs::attributes;
use crate::color::Color;
use crate::figure::{BlockId, Dirty, FigShared, GridPosition, PlotId};
use crate::style::HAlign;
use crate::text::{Font, RichText};
use crate::transform::Scale;
use std::sync::Arc;

/// Makie's `Axis`. Create one at a grid position; plot into it with its methods or the `!` macros.
///
/// ```
/// use ezviz::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1)).title("damped").xlabel("t (s)").ylabel("u (V)");
/// ax.scatter(&[0.0, 1.0, 2.0], &[1.0, 0.5, 0.25]);
/// ```
#[derive(Clone)]
pub struct Axis {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: BlockId,
}

impl PartialEq for Axis {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.sh, &other.sh) && self.id == other.id
    }
}

impl std::fmt::Debug for Axis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Axis#{}", self.id.index)
    }
}

/// Limits requested by the user or by interaction, per dimension, in data coordinates.
#[derive(Clone, Debug, Default)]
pub(crate) struct AxisState {
    pub attrs: AxisAttrs,
    pub plots: Vec<PlotId>,
    /// `xlims!`/`ylims!` values (None = automatic).
    pub xlims: (Option<f64>, Option<f64>),
    pub ylims: (Option<f64>, Option<f64>),
    /// Limits set by interaction (pan/zoom), `[x0, x1, y0, y1]` in data space. Cleared by reset.
    pub interactive: Option<[f64; 4]>,
    pub xlinks: Vec<BlockId>,
    pub ylinks: Vec<BlockId>,
    /// Re-autoscale on new data even after the user zoomed (live data).
    pub follow: bool,
}

attributes! {
    Axis(AxisAttrs, AxisResolved, AxisTheme) via with_attrs {
        /// Title above the axis.
        title: RichText = |_| RichText::default(), LAYOUT;
        titlesize: f64 = |g| g.fontsize, LAYOUT;
        titlefont: Font = |_| Font::Bold, LAYOUT;
        titlecolor: Color = |g| g.textcolor, STYLE;
        titlegap: f64 = |_| 4.0, LAYOUT;
        titlealign: HAlign = |_| HAlign::Center, STYLE;
        titlevisible: bool = |_| true, LAYOUT;
        subtitle: RichText = |_| RichText::default(), LAYOUT;
        subtitlesize: f64 = |g| g.fontsize, LAYOUT;
        subtitlegap: f64 = |_| 0.0, LAYOUT;
        subtitlecolor: Color = |g| g.textcolor, STYLE;
        xlabel: RichText = |_| RichText::default(), LAYOUT;
        ylabel: RichText = |_| RichText::default(), LAYOUT;
        xlabelsize: f64 = |g| g.fontsize, LAYOUT;
        ylabelsize: f64 = |g| g.fontsize, LAYOUT;
        xlabelcolor: Color = |g| g.textcolor, STYLE;
        ylabelcolor: Color = |g| g.textcolor, STYLE;
        xlabelpadding: f64 = |_| 3.0, LAYOUT;
        ylabelpadding: f64 = |_| 5.0, LAYOUT;
        xlabelvisible: bool = |_| true, LAYOUT;
        ylabelvisible: bool = |_| true, LAYOUT;
        xlabelfont: Font = |_| Font::Regular, LAYOUT;
        ylabelfont: Font = |_| Font::Regular, LAYOUT;
        xticklabelsize: f64 = |g| g.fontsize, LAYOUT;
        yticklabelsize: f64 = |g| g.fontsize, LAYOUT;
        xticklabelcolor: Color = |g| g.textcolor, STYLE;
        yticklabelcolor: Color = |g| g.textcolor, STYLE;
        xticklabelpad: f64 = |_| 2.0, LAYOUT;
        yticklabelpad: f64 = |_| 4.0, LAYOUT;
        xticklabelsvisible: bool = |_| true, LAYOUT;
        yticklabelsvisible: bool = |_| true, LAYOUT;
        xticklabelfont: Font = |_| Font::Regular, LAYOUT;
        yticklabelfont: Font = |_| Font::Regular, LAYOUT;
        xticklabelrotation: f64 = |_| 0.0, LAYOUT;
        yticklabelrotation: f64 = |_| 0.0, LAYOUT;
        /// Fixed room for y tick labels (units), so the layout doesn't jitter as labels change.
        xticklabelspace: Option<f64> = |_| None, LAYOUT;
        yticklabelspace: Option<f64> = |_| None, LAYOUT;
        xticksvisible: bool = |_| true, LAYOUT;
        yticksvisible: bool = |_| true, LAYOUT;
        xticksize: f64 = |_| 5.0, LAYOUT;
        yticksize: f64 = |_| 5.0, LAYOUT;
        xtickwidth: f64 = |_| 1.0, STYLE;
        ytickwidth: f64 = |_| 1.0, STYLE;
        /// 0 = outward, 1 = inward.
        xtickalign: f64 = |_| 0.0, LAYOUT;
        ytickalign: f64 = |_| 0.0, LAYOUT;
        xtickcolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        ytickcolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        xticksmirrored: bool = |_| false, STYLE;
        yticksmirrored: bool = |_| false, STYLE;
        xminorticksvisible: bool = |_| false, STYLE;
        yminorticksvisible: bool = |_| false, STYLE;
        xminorticksize: f64 = |_| 3.0, STYLE;
        yminorticksize: f64 = |_| 3.0, STYLE;
        xminortickwidth: f64 = |_| 1.0, STYLE;
        yminortickwidth: f64 = |_| 1.0, STYLE;
        xminortickcolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        yminortickcolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        xgridvisible: bool = |_| true, STYLE;
        ygridvisible: bool = |_| true, STYLE;
        xgridcolor: Color = |_| Color::rgba(0.0, 0.0, 0.0, 0.12), STYLE;
        ygridcolor: Color = |_| Color::rgba(0.0, 0.0, 0.0, 0.12), STYLE;
        xgridwidth: f64 = |_| 1.0, STYLE;
        ygridwidth: f64 = |_| 1.0, STYLE;
        xminorgridvisible: bool = |_| false, STYLE;
        yminorgridvisible: bool = |_| false, STYLE;
        xminorgridcolor: Color = |_| Color::rgba(0.0, 0.0, 0.0, 0.05), STYLE;
        yminorgridcolor: Color = |_| Color::rgba(0.0, 0.0, 0.0, 0.05), STYLE;
        xminorgridwidth: f64 = |_| 1.0, STYLE;
        yminorgridwidth: f64 = |_| 1.0, STYLE;
        spinewidth: f64 = |_| 1.0, STYLE;
        leftspinevisible: bool = |_| true, STYLE;
        rightspinevisible: bool = |_| true, STYLE;
        bottomspinevisible: bool = |_| true, STYLE;
        topspinevisible: bool = |_| true, STYLE;
        leftspinecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        rightspinecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        bottomspinecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        topspinecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        backgroundcolor: Color = |_| Color::rgb(1.0, 1.0, 1.0), STYLE;
        /// Relative margin added to automatic limits, `(low, high)`.
        xautolimitmargin: [f64; 2] = |_| [0.05, 0.05], LIMITS;
        yautolimitmargin: [f64; 2] = |_| [0.05, 0.05], LIMITS;
        xreversed: bool = |_| false, LIMITS;
        yreversed: bool = |_| false, LIMITS;
        xscale: Scale = |_| Scale::Identity, LIMITS;
        yscale: Scale = |_| Scale::Identity, LIMITS;
        /// Fixed width of the axis area in units (None = fill the cell).
        width: Option<f64> = |_| None, LAYOUT;
        /// Fixed height of the axis area in units (None = fill the cell).
        height: Option<f64> = |_| None, LAYOUT;
    }
}

impl Axis {
    /// Makie's `Axis(fig[r, c])`.
    #[track_caller]
    pub fn new(pos: GridPosition) -> Axis {
        let sh = pos.fig.sh.clone();
        let id = sh.update(Dirty::LAYOUT, |st| {
            let place = pos.resolve(st);
            st.add_block(place, crate::blocks::Block::Axis(Box::default()))
        });
        Axis { sh, id }
    }

    pub(crate) fn with_state<R>(&self, dirty: u8, f: impl FnOnce(&mut AxisState) -> R) -> Option<R> {
        self.sh.update(dirty, |st| st.block_mut(self.id).and_then(|b| b.as_axis_mut()).map(f))
    }

    fn with_attrs(&self, f: impl FnOnce(&mut AxisAttrs), dirty: u8) {
        if self.with_state(dirty, |a| f(&mut a.attrs)).is_none() {
            crate::warn_once("setter called on an Axis that no longer exists");
        }
    }

    /// The figure this axis belongs to.
    pub fn figure(&self) -> crate::Figure {
        crate::Figure { sh: self.sh.clone() }
    }

    /// Makie's `xlims!(ax, lo, hi)`. `None` leaves that side automatic; `lo > hi` reverses.
    pub fn xlims(&self, lo: impl crate::attrs::Conv<Option<f64>>, hi: impl crate::attrs::Conv<Option<f64>>) -> Axis {
        let (lo, hi) = (lo.conv(), hi.conv());
        self.with_state(Dirty::LIMITS, |a| {
            a.xlims = (lo, hi);
            a.interactive = None;
        });
        self.clone()
    }

    /// Makie's `ylims!(ax, lo, hi)`.
    pub fn ylims(&self, lo: impl crate::attrs::Conv<Option<f64>>, hi: impl crate::attrs::Conv<Option<f64>>) -> Axis {
        let (lo, hi) = (lo.conv(), hi.conv());
        self.with_state(Dirty::LIMITS, |a| {
            a.ylims = (lo, hi);
            a.interactive = None;
        });
        self.clone()
    }

    /// Makie's `limits!(ax, x1, x2, y1, y2)`.
    pub fn limits(&self, x1: f64, x2: f64, y1: f64, y2: f64) -> Axis {
        self.with_state(Dirty::LIMITS, |a| {
            a.xlims = (Some(x1), Some(x2));
            a.ylims = (Some(y1), Some(y2));
            a.interactive = None;
        });
        self.clone()
    }

    /// Makie's `reset_limits!`: forget interactive zoom/pan (keeps `xlims`/`ylims`).
    pub fn reset_limits(&self) -> Axis {
        self.with_state(Dirty::LIMITS, |a| a.interactive = None);
        self.clone()
    }

    /// Makie's `autolimits!`: forget all fixed and interactive limits.
    pub fn autolimits(&self) -> Axis {
        self.with_state(Dirty::LIMITS, |a| {
            a.interactive = None;
            a.xlims = (None, None);
            a.ylims = (None, None);
        });
        self.clone()
    }

    /// Keep autoscaling to new data (live plots), even after the user pans or zooms.
    pub fn follow(&self, on: bool) -> Axis {
        self.with_state(Dirty::LIMITS, |a| a.follow = on);
        self.clone()
    }

    /// Makie's `hidexdecorations!`: hides x ticks, tick labels and label (and grid if `grid`).
    pub fn hidexdecorations(&self, grid: bool) -> Axis {
        self.xlabelvisible(false).xticklabelsvisible(false).xticksvisible(false).xminorticksvisible(false);
        if grid {
            self.xgridvisible(false).xminorgridvisible(false);
        }
        self.clone()
    }

    /// Makie's `hideydecorations!`.
    pub fn hideydecorations(&self, grid: bool) -> Axis {
        self.ylabelvisible(false).yticklabelsvisible(false).yticksvisible(false).yminorticksvisible(false);
        if grid {
            self.ygridvisible(false).yminorgridvisible(false);
        }
        self.clone()
    }

    /// Makie's `hidedecorations!`.
    pub fn hidedecorations(&self, grid: bool) -> Axis {
        self.hidexdecorations(grid).hideydecorations(grid)
    }

    /// Makie's `hidespines!` (all four).
    pub fn hidespines(&self) -> Axis {
        self.leftspinevisible(false).rightspinevisible(false).bottomspinevisible(false).topspinevisible(false)
    }
}

fn link(axes: &[&Axis], x: bool, y: bool) {
    let Some(first) = axes.first() else { return };
    for a in axes {
        assert!(Arc::ptr_eq(&a.sh, &first.sh), "linked axes must belong to the same figure");
    }
    let ids: Vec<BlockId> = axes.iter().map(|a| a.id).collect();
    first.sh.update(Dirty::LIMITS, |st| {
        for &id in &ids {
            if let Some(ax) = st.block_mut(id).and_then(|b| b.as_axis_mut()) {
                for &other in &ids {
                    if other != id {
                        if x && !ax.xlinks.contains(&other) {
                            ax.xlinks.push(other);
                        }
                        if y && !ax.ylinks.contains(&other) {
                            ax.ylinks.push(other);
                        }
                    }
                }
            }
        }
    });
}

/// Makie's `linkxaxes!`: the axes share x limits.
pub fn linkxaxes(axes: &[&Axis]) {
    link(axes, true, false)
}

/// Makie's `linkyaxes!`.
pub fn linkyaxes(axes: &[&Axis]) {
    link(axes, false, true)
}

/// Makie's `linkaxes!`: both dimensions.
pub fn linkaxes(axes: &[&Axis]) {
    link(axes, true, true)
}
