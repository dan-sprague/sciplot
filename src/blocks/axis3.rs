//! `Axis3`: Makie's 3D axis (a box with panels, grids, ticks and labels on the edges facing the
//! viewer, seen by an orbit camera).
//!
//! Layout, decorations and plots are lowered by [`crate::scene::axis3`]; the camera math lives in
//! [`crate::scene::axis3::camera`].

use super::{BlockCtx, BlockImpl, BlockLayout};
use crate::attrs::{Conv, attributes, conv_identity};
use crate::color::Color;
use crate::figure::{BlockId, Dirty, FigShared, GridPosition, PlotId};
use crate::scene::drawlist::{Emitter, Rect};
use crate::style::{HAlign, VAlign};
use crate::text::{Font, RichText};
use crate::ticks::{TickFormat, TickSpec};
use std::sync::Arc;

pub use crate::scene::axis3::camera::{Aspect3, ViewMode};

/// Makie's `Axis3`: a 3D axis. Plot into it with [`Axis3::lines`], [`Axis3::scatter`] and
/// [`Axis3::surface`]; rotate it with [`Axis3::azimuth`] / [`Axis3::elevation`] (or by dragging in
/// a window).
///
/// ```no_run
/// use ezviz::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis3::new(fig.at(1, 1)).title("helix").azimuth(0.3 * std::f64::consts::PI);
/// let t = linspace(0.0, 20.0, 500);
/// ax.lines(t.iter().map(|t| t.cos()), t.iter().map(|t| t.sin()), &t);
/// fig.save("helix.png").unwrap();
/// ```
#[derive(Clone)]
pub struct Axis3 {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: BlockId,
}

impl PartialEq for Axis3 {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.sh, &other.sh) && self.id == other.id
    }
}

impl std::fmt::Debug for Axis3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Axis3#{}", self.id.index)
    }
}

/// An Axis3's state.
#[derive(Clone, Debug)]
pub(crate) struct Axis3State {
    pub attrs: Axis3Attrs,
    pub plots: Vec<PlotId>,
    /// `xlims!`/`ylims!`/`zlims!` values `[x0, x1, y0, y1, z0, z1]` (None = automatic).
    pub limits: [Option<f64>; 6],
    /// Scroll zoom (Makie's `zoom_mult`; 1 = none).
    pub zoom_mult: f64,
    /// `viewmode = Free` translation (Makie's `axis_offset`).
    pub offset: [f64; 2],
}

impl Default for Axis3State {
    fn default() -> Self {
        Axis3State { attrs: Axis3Attrs::default(), plots: Vec::new(), limits: [None; 6], zoom_mult: 1.0, offset: [0.0; 2] }
    }
}

conv_identity!(Aspect3, ViewMode);

impl Conv<Aspect3> for crate::DataAspect {
    fn conv(self) -> Aspect3 {
        Aspect3::Data
    }
}
/// `(a, b, c)`: side lengths in this ratio.
impl<A: crate::data::Num, B: crate::data::Num, C: crate::data::Num> Conv<Aspect3> for (A, B, C) {
    fn conv(self) -> Aspect3 {
        Aspect3::Ratio(self.0.to_f64(), self.1.to_f64(), self.2.to_f64())
    }
}

attributes! {
    Axis3(Axis3Attrs, Axis3Resolved, Axis3Theme) via with_attrs {
        /// Camera elevation above the xy plane in radians (Makie default π/8).
        elevation: f64 = |_| std::f64::consts::PI / 8.0, LAYOUT;
        /// Camera azimuth in radians: 0 looks from +x, rotating counter-clockwise seen from above
        /// (Makie default 1.275π).
        azimuth: f64 = |_| 1.275 * std::f64::consts::PI, LAYOUT;
        /// 0 = (nearly) orthographic, 1 = 90° field of view.
        perspectiveness: f64 = |_| 0.0, LAYOUT;
        /// Near clip distance (Makie's `near`).
        near: f64 = |_| 1e-3, LAYOUT;
        /// Box proportions: [`Aspect3::Ratio`] (default `(1, 1, 2/3)`), [`Aspect3::Data`] or
        /// [`Aspect3::Equal`]; a tuple `(a, b, c)` converts.
        aspect: Aspect3 = |_| Aspect3::Ratio(1.0, 1.0, 2.0 / 3.0), LAYOUT;
        /// How the box is fitted into the axis area (default [`ViewMode::FitZoom`]).
        viewmode: ViewMode = |_| ViewMode::FitZoom, LAYOUT;
        /// Hide plot content outside the limits box (Makie's `clip`).
        clip: bool = |_| true, STYLE;
        /// Background of the scene area (transparent by default).
        backgroundcolor: Color = |_| Color::TRANSPARENT, STYLE;
        title: RichText = |_| RichText::default(), LAYOUT;
        titlesize: f64 = |g| g.fontsize, LAYOUT;
        titlefont: Font = |_| Font::Bold, LAYOUT;
        titlecolor: Color = |g| g.textcolor, STYLE;
        titlegap: f64 = |_| 4.0, LAYOUT;
        titlealign: HAlign = |_| HAlign::Center, STYLE;
        titlevisible: bool = |_| true, LAYOUT;
        xlabel: RichText = |_| RichText::from("x"), STYLE;
        ylabel: RichText = |_| RichText::from("y"), STYLE;
        zlabel: RichText = |_| RichText::from("z"), STYLE;
        xlabelsize: f64 = |g| g.fontsize, STYLE;
        ylabelsize: f64 = |g| g.fontsize, STYLE;
        zlabelsize: f64 = |g| g.fontsize, STYLE;
        xlabelcolor: Color = |g| g.textcolor, STYLE;
        ylabelcolor: Color = |g| g.textcolor, STYLE;
        zlabelcolor: Color = |g| g.textcolor, STYLE;
        xlabelfont: Font = |_| Font::Regular, STYLE;
        ylabelfont: Font = |_| Font::Regular, STYLE;
        zlabelfont: Font = |_| Font::Regular, STYLE;
        xlabelvisible: bool = |_| true, STYLE;
        ylabelvisible: bool = |_| true, STYLE;
        zlabelvisible: bool = |_| true, STYLE;
        /// Label rotation in radians (`None` = Makie's automatic: along the axis, upright).
        xlabelrotation: Option<f64> = |_| None, STYLE;
        ylabelrotation: Option<f64> = |_| None, STYLE;
        zlabelrotation: Option<f64> = |_| None, STYLE;
        /// Distance of the label from the axis line in units.
        xlabeloffset: f64 = |_| 40.0, STYLE;
        ylabeloffset: f64 = |_| 40.0, STYLE;
        zlabeloffset: f64 = |_| 50.0, STYLE;
        xticklabelsize: f64 = |g| g.fontsize, STYLE;
        yticklabelsize: f64 = |g| g.fontsize, STYLE;
        zticklabelsize: f64 = |g| g.fontsize, STYLE;
        xticklabelcolor: Color = |g| g.textcolor, STYLE;
        yticklabelcolor: Color = |g| g.textcolor, STYLE;
        zticklabelcolor: Color = |g| g.textcolor, STYLE;
        xticklabelfont: Font = |_| Font::Regular, STYLE;
        yticklabelfont: Font = |_| Font::Regular, STYLE;
        zticklabelfont: Font = |_| Font::Regular, STYLE;
        xticklabelsvisible: bool = |_| true, STYLE;
        yticklabelsvisible: bool = |_| true, STYLE;
        zticklabelsvisible: bool = |_| true, STYLE;
        /// Gap between the tick end and its label in units.
        xticklabelpad: f64 = |_| 5.0, STYLE;
        yticklabelpad: f64 = |_| 5.0, STYLE;
        zticklabelpad: f64 = |_| 10.0, STYLE;
        /// Major ticks (default Makie's `WilkinsonTicks(5; k_min = 3)`).
        xticks: TickSpec = |_| TickSpec::Automatic, LIMITS;
        yticks: TickSpec = |_| TickSpec::Automatic, LIMITS;
        zticks: TickSpec = |_| TickSpec::Automatic, LIMITS;
        xtickformat: TickFormat = |_| TickFormat::Automatic, STYLE;
        ytickformat: TickFormat = |_| TickFormat::Automatic, STYLE;
        ztickformat: TickFormat = |_| TickFormat::Automatic, STYLE;
        xticksvisible: bool = |_| true, STYLE;
        yticksvisible: bool = |_| true, STYLE;
        zticksvisible: bool = |_| true, STYLE;
        xticksize: f64 = |_| 6.0, STYLE;
        yticksize: f64 = |_| 6.0, STYLE;
        zticksize: f64 = |_| 6.0, STYLE;
        xtickwidth: f64 = |_| 1.0, STYLE;
        ytickwidth: f64 = |_| 1.0, STYLE;
        ztickwidth: f64 = |_| 1.0, STYLE;
        xtickcolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        ytickcolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        ztickcolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        xgridvisible: bool = |_| true, STYLE;
        ygridvisible: bool = |_| true, STYLE;
        zgridvisible: bool = |_| true, STYLE;
        xgridcolor: Color = |_| Color::rgba(0.0, 0.0, 0.0, 0.12), STYLE;
        ygridcolor: Color = |_| Color::rgba(0.0, 0.0, 0.0, 0.12), STYLE;
        zgridcolor: Color = |_| Color::rgba(0.0, 0.0, 0.0, 0.12), STYLE;
        xgridwidth: f64 = |_| 1.0, STYLE;
        ygridwidth: f64 = |_| 1.0, STYLE;
        zgridwidth: f64 = |_| 1.0, STYLE;
        xspinesvisible: bool = |_| true, STYLE;
        yspinesvisible: bool = |_| true, STYLE;
        zspinesvisible: bool = |_| true, STYLE;
        xspinecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        yspinecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        zspinecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        xspinewidth: f64 = |_| 1.0, STYLE;
        yspinewidth: f64 = |_| 1.0, STYLE;
        zspinewidth: f64 = |_| 1.0, STYLE;
        /// Also draw the three box edges nearest the viewer (Makie's `front_spines`).
        front_spines: bool = |_| false, STYLE;
        xypanelcolor: Color = |_| Color::TRANSPARENT, STYLE;
        yzpanelcolor: Color = |_| Color::TRANSPARENT, STYLE;
        xzpanelcolor: Color = |_| Color::TRANSPARENT, STYLE;
        xypanelvisible: bool = |_| true, STYLE;
        yzpanelvisible: bool = |_| true, STYLE;
        xzpanelvisible: bool = |_| true, STYLE;
        /// Room reserved around the box for labels, `(left, right, bottom, top)` (Makie: 30 on
        /// every side; Axis3 does not compute it from its labels).
        protrusions: [f64; 4] = |_| [30.0; 4], LAYOUT;
        xautolimitmargin: [f64; 2] = |_| [0.05, 0.05], LIMITS;
        yautolimitmargin: [f64; 2] = |_| [0.05, 0.05], LIMITS;
        zautolimitmargin: [f64; 2] = |_| [0.05, 0.05], LIMITS;
        xreversed: bool = |_| false, LIMITS;
        yreversed: bool = |_| false, LIMITS;
        zreversed: bool = |_| false, LIMITS;
        /// Fixed width of the axis area in units (None = fill the cell).
        width: Option<f64> = |_| None, LAYOUT;
        /// Fixed height of the axis area in units (None = fill the cell).
        height: Option<f64> = |_| None, LAYOUT;
        tellwidth: bool = |_| true, LAYOUT;
        tellheight: bool = |_| true, LAYOUT;
        halign: HAlign = |_| HAlign::Center, LAYOUT;
        valign: VAlign = |_| VAlign::Center, LAYOUT;
    }
}

impl Axis3 {
    /// Makie's `Axis3(fig[r, c])`.
    #[track_caller]
    pub fn new(pos: GridPosition) -> Axis3 {
        let sh = pos.fig.sh.clone();
        let id = sh.update(Dirty::LAYOUT, |st| {
            let place = pos.resolve(st);
            st.add_block(place, super::Block::Axis3(Box::default()))
        });
        Axis3 { sh, id }
    }

    pub(crate) fn with_state<R>(&self, dirty: u8, f: impl FnOnce(&mut Axis3State) -> R) -> Option<R> {
        let r = self.sh.update(dirty, |st| match st.block_mut(self.id) {
            Some(super::Block::Axis3(a)) => Some(f(a)),
            _ => None,
        });
        if r.is_none() {
            crate::warn_once("setter called on an Axis3 that no longer exists");
        }
        r
    }

    fn with_attrs(&self, f: impl FnOnce(&mut Axis3Attrs), dirty: u8) {
        self.with_state(dirty, |a| f(&mut a.attrs));
    }

    /// The figure this axis belongs to.
    pub fn figure(&self) -> crate::Figure {
        crate::Figure { sh: self.sh.clone() }
    }

    /// Removes the axis and its plots from the figure.
    pub fn delete(&self) {
        let id = self.id;
        self.sh.update(Dirty::LAYOUT, |st| {
            let plots = match st.block(id) {
                Some(super::Block::Axis3(a)) => a.plots.clone(),
                _ => return,
            };
            for p in plots {
                if let Some(s) = st.plots.get_mut(p.index as usize) {
                    *s = None;
                }
            }
            st.blocks[id.index as usize] = None;
        });
    }

    fn set_lims(&self, i: usize, lo: Option<f64>, hi: Option<f64>) -> Axis3 {
        self.with_state(Dirty::LIMITS, |a| {
            a.limits[2 * i] = lo;
            a.limits[2 * i + 1] = hi;
        });
        self.clone()
    }

    /// Makie's `xlims!(ax, lo, hi)`: `None` leaves that side automatic.
    pub fn xlims(&self, lo: impl Conv<Option<f64>>, hi: impl Conv<Option<f64>>) -> Axis3 {
        self.set_lims(0, lo.conv(), hi.conv())
    }
    /// Makie's `ylims!(ax, lo, hi)`.
    pub fn ylims(&self, lo: impl Conv<Option<f64>>, hi: impl Conv<Option<f64>>) -> Axis3 {
        self.set_lims(1, lo.conv(), hi.conv())
    }
    /// Makie's `zlims!(ax, lo, hi)`.
    pub fn zlims(&self, lo: impl Conv<Option<f64>>, hi: impl Conv<Option<f64>>) -> Axis3 {
        self.set_lims(2, lo.conv(), hi.conv())
    }
    /// Makie's `limits!(ax, x1, x2, y1, y2, z1, z2)`.
    pub fn limits(&self, x1: f64, x2: f64, y1: f64, y2: f64, z1: f64, z2: f64) -> Axis3 {
        self.with_state(Dirty::LIMITS, |a| a.limits = [x1, x2, y1, y2, z1, z2].map(Some));
        self.clone()
    }
    /// Makie's `autolimits!`: forget all fixed limits.
    pub fn autolimits(&self) -> Axis3 {
        self.with_state(Dirty::LIMITS, |a| a.limits = [None; 6]);
        self.clone()
    }

    /// Rotates the camera like dragging in a window: `dx`, `dy` in units (see
    /// [`crate::scene::axis3::drag_rotate`]).
    pub fn rotate_by(&self, dx: f64, dy: f64) -> Axis3 {
        let (az, el) = {
            let st = self.sh.state.lock();
            let g = st.theme.globals();
            match st.block(self.id) {
                Some(super::Block::Axis3(a)) => {
                    let r = a.attrs.resolve(&st.theme.axis3, &g);
                    (r.azimuth, r.elevation)
                }
                _ => return self.clone(),
            }
        };
        let (az, el) = crate::scene::axis3::drag_rotate(az, el, dx, dy);
        self.azimuth(az).elevation(el)
    }

    /// Zooms like scrolling in a window: `amount > 0` zooms in (Makie's `ScrollZoom(0.05)`: the
    /// zoom multiplier is scaled by `(1 - 0.05)^amount`).
    pub fn zoom_by(&self, amount: f64) -> Axis3 {
        self.with_state(Dirty::LAYOUT, |a| a.zoom_mult = crate::scene::axis3::scroll_zoom(a.zoom_mult, amount));
        self.clone()
    }

    /// Resets zoom and translation (Makie's `LimitReset`).
    pub fn reset_view(&self) -> Axis3 {
        self.with_state(Dirty::LAYOUT, |a| {
            a.zoom_mult = 1.0;
            a.offset = [0.0; 2];
        });
        self.clone()
    }

    /// Makie's `hidedecorations!`: hides labels, tick labels, ticks and grids.
    pub fn hidedecorations(&self) -> Axis3 {
        self.xlabelvisible(false).ylabelvisible(false).zlabelvisible(false);
        self.xticklabelsvisible(false).yticklabelsvisible(false).zticklabelsvisible(false);
        self.xticksvisible(false).yticksvisible(false).zticksvisible(false);
        self.xgridvisible(false).ygridvisible(false).zgridvisible(false)
    }

    /// Makie's `hidespines!`.
    pub fn hidespines(&self) -> Axis3 {
        self.xspinesvisible(false).yspinesvisible(false).zspinesvisible(false)
    }

    /// The axis as laid out right now (for fidelity checks against Makie); `None` if deleted.
    #[doc(hidden)]
    pub fn geometry(&self) -> Option<crate::scene::axis3::Axis3Geometry> {
        let st = self.sh.snapshot();
        let mut cache = crate::scene::SceneCache::new();
        crate::scene::build(&st, None, &mut cache);
        crate::scene::axis3::geometry(&cache, self.id)
    }
}

impl BlockImpl for Axis3State {
    fn layout(&self, ctx: &BlockCtx<'_>) -> BlockLayout {
        let r = self.attrs.resolve(&ctx.st.theme.axis3, ctx.g);
        let [l, rt, b, t] = r.protrusions;
        BlockLayout {
            protrusion: crate::layout::Protrusion { left: l, right: rt, bottom: b, top: t },
            width: r.width.into(),
            height: r.height.into(),
            autosize: [None, None],
            tellwidth: r.tellwidth,
            tellheight: r.tellheight,
            halign: r.halign.frac(),
            valign: r.valign.frac(),
            ..Default::default()
        }
    }

    /// Drawn by `scene::axis3::emit` (it needs the scene cache); nothing here.
    fn emit(&self, _ctx: &BlockCtx<'_>, _em: &mut Emitter, _rect: Rect) {}
}
