//! `Legend` and `axislegend` (Makie's `Legend` block, `ML/blocks/legend.jl`).
//!
//! A legend lists labelled plots: each entry is a patch (`patchsize`, 20 × 20 by default) holding
//! the plot's legend elements (a line, a marker, a filled rectangle, ...) and a label. Entries are
//! collected from the source axes at every frame, so plots labelled later appear automatically.
//!
//! Layout (Makie's inner `GridLayout` with `Outside(padding)`): an optional title row (`titlegap`
//! below it), then the entries in `[patch | label]` column pairs (`patchlabelgap` between patch
//! and label, `colgap` between banks, `rowgap` between rows). The legend's size is that grid plus
//! `padding` and `margin`; the frame and background fill the area inside the margin.

use super::{BlockCtx, BlockImpl, BlockLayout, block_common};
use crate::attrs::{attributes, conv_identity};
use crate::color::Color;
use crate::figure::{BlockId, Dirty, FigShared, FigState, GridPosition, PlotId};
use crate::layout::{BlockSize, Protrusion};
use crate::plots::LegendElement;
use crate::plots::legend_elements::LegendCtx;
use crate::scene::drawlist::{
    Buf, Emitter, GlyphsPrim, LinesPrim, MarkersPrim, MeshPrim, MeshVertex, Prim, PrimColor, Rect, RectPrim, Space,
};
use crate::style::{HAlign, JoinStyle, LineCap, VAlign};
use crate::text::{Font, RichText, TextLayout};
use crate::theme::Globals;
use std::collections::HashMap;
use std::sync::Arc;

/// A legend block (Makie's `Legend`): in a grid cell with [`Legend::new`] /
/// [`Legend::from_entries`], or over an axis' plot area with [`axislegend`].
///
/// ```
/// use sciplot::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// let t = linspace(0.0, 10.0, 100);
/// lines!(ax, &t, t.iter().map(|t| t.sin()); label = "sin");
/// scatter!(ax, &t, t.iter().map(|t| t.cos()); label = "cos");
/// Legend!(fig.at(1, 2), &ax; title = "Functions");
/// ```
#[derive(Clone)]
pub struct Legend {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: BlockId,
}

/// Legend orientation (Makie's `:vertical` / `:horizontal`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Orientation {
    /// Entries stacked top to bottom (`nbanks` columns). The legend tells its width to the layout.
    #[default]
    Vertical,
    /// Entries side by side (`nbanks` rows). The legend tells its height to the layout.
    Horizontal,
}

conv_identity!(Orientation);

/// Where [`axislegend`] puts the legend inside the axis (Makie's `position = :rt` etc.): the
/// first letter is the horizontal side (Left, Center, Right), the second the vertical one (Top,
/// Center, Bottom). `Frac(h, v)` gives fractions (0 = left/bottom, 1 = right/top).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pos {
    LT,
    CT,
    RT,
    LC,
    CC,
    RC,
    LB,
    CB,
    RB,
    Frac(f64, f64),
}

impl Pos {
    /// Makie's `legend_position_to_aligns`.
    pub fn aligns(self) -> (HAlign, VAlign) {
        use {HAlign as H, VAlign as V};
        match self {
            Pos::LT => (H::Left, V::Top),
            Pos::CT => (H::Center, V::Top),
            Pos::RT => (H::Right, V::Top),
            Pos::LC => (H::Left, V::Center),
            Pos::CC => (H::Center, V::Center),
            Pos::RC => (H::Right, V::Center),
            Pos::LB => (H::Left, V::Bottom),
            Pos::CB => (H::Center, V::Bottom),
            Pos::RB => (H::Right, V::Bottom),
            Pos::Frac(h, v) => (H::Frac(h), V::Frac(v)),
        }
    }
}

/// Legend entries name plots through the crate-wide [`PlotRef`] (re-exported here for the
/// `blocks` API path).
pub use crate::plots::PlotRef;

/// One legend entry: a label with the plots (their elements are layered in one patch) and/or
/// explicit [`LegendElement`]s.
///
/// ```
/// use sciplot::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// let l = ax.lines([1.0, 2.0], [1.0, 2.0]);
/// let s = ax.scatter([1.0, 2.0], [2.0, 1.0]);
/// Legend::from_entries(fig.at(1, 2), [
///     LegendEntry::new("line", &l),
///     LegendEntry::merged("both", [PlotRef::from(&l), PlotRef::from(&s)]),
///     LegendEntry::elements("custom", [LegendElement::Poly {
///         color: RED, strokecolor: BLACK, strokewidth: 1.0,
///     }]),
/// ]);
/// ```
#[derive(Clone, Debug)]
pub struct LegendEntry {
    label: RichText,
    plots: Vec<PlotRef>,
    elements: Vec<LegendElement>,
}

impl LegendEntry {
    /// An entry showing one plot.
    pub fn new(label: impl Into<RichText>, plot: impl Into<PlotRef>) -> LegendEntry {
        LegendEntry { label: label.into(), plots: vec![plot.into()], elements: Vec::new() }
    }

    /// An entry layering several plots in one patch (what `merge = true` does per label).
    pub fn merged(label: impl Into<RichText>, plots: impl IntoIterator<Item = PlotRef>) -> LegendEntry {
        LegendEntry { label: label.into(), plots: plots.into_iter().collect(), elements: Vec::new() }
    }

    /// An entry drawing explicit elements (Makie's `LineElement`, `MarkerElement`, ...).
    pub fn elements(label: impl Into<RichText>, elements: impl IntoIterator<Item = LegendElement>) -> LegendEntry {
        LegendEntry { label: label.into(), plots: Vec::new(), elements: elements.into_iter().collect() }
    }
}

impl<P: Into<PlotRef>, L: Into<RichText>> From<(P, L)> for LegendEntry {
    fn from((p, l): (P, L)) -> LegendEntry {
        LegendEntry::new(l, p)
    }
}

/// The axes a legend collects its entries from: `&ax`, `&[&a, &b]`, `[&a, &b]`, `&vec_of_axes`...
pub trait LegendSource {
    #[doc(hidden)]
    fn __axes(self) -> Vec<crate::Axis>;
}

impl LegendSource for crate::Axis {
    fn __axes(self) -> Vec<crate::Axis> {
        vec![self]
    }
}
impl LegendSource for &crate::Axis {
    fn __axes(self) -> Vec<crate::Axis> {
        vec![self.clone()]
    }
}
impl LegendSource for &[&crate::Axis] {
    fn __axes(self) -> Vec<crate::Axis> {
        self.iter().map(|a| (*a).clone()).collect()
    }
}
impl LegendSource for &[crate::Axis] {
    fn __axes(self) -> Vec<crate::Axis> {
        self.to_vec()
    }
}
impl LegendSource for Vec<crate::Axis> {
    fn __axes(self) -> Vec<crate::Axis> {
        self
    }
}
impl LegendSource for &Vec<crate::Axis> {
    fn __axes(self) -> Vec<crate::Axis> {
        self.clone()
    }
}
impl LegendSource for Vec<&crate::Axis> {
    fn __axes(self) -> Vec<crate::Axis> {
        self.into_iter().cloned().collect()
    }
}
impl LegendSource for &Vec<&crate::Axis> {
    fn __axes(self) -> Vec<crate::Axis> {
        self.iter().map(|a| (*a).clone()).collect()
    }
}
impl<const N: usize> LegendSource for [&crate::Axis; N] {
    fn __axes(self) -> Vec<crate::Axis> {
        self.into_iter().cloned().collect()
    }
}
impl<const N: usize> LegendSource for &[&crate::Axis; N] {
    fn __axes(self) -> Vec<crate::Axis> {
        self.iter().map(|a| (*a).clone()).collect()
    }
}
impl<const N: usize> LegendSource for [crate::Axis; N] {
    fn __axes(self) -> Vec<crate::Axis> {
        self.into_iter().collect()
    }
}
impl<const N: usize> LegendSource for &[crate::Axis; N] {
    fn __axes(self) -> Vec<crate::Axis> {
        self.to_vec()
    }
}

/// An entry as stored in the figure (plots by id).
#[derive(Clone, Debug)]
struct StoredEntry {
    label: RichText,
    plots: Vec<PlotId>,
    elements: Vec<LegendElement>,
}

#[derive(Clone, Debug)]
enum Source {
    /// Every labelled plot of these axes, in order (Makie's `get_labeled_plots`).
    Axes(Vec<BlockId>),
    Entries(Vec<StoredEntry>),
}

#[derive(Clone, Debug)]
pub(crate) struct LegendState {
    source: Source,
    /// Layer plots with the same label into one entry.
    merge: bool,
    /// Keep only the first plot of each (plot type, label).
    unique: bool,
    nbanks: usize,
    /// `axislegend`: drawn over this axis' plot area instead of taking a grid cell.
    inside: Option<BlockId>,
    pub attrs: LegendAttrs,
}

attributes! {
    Legend(LegendAttrs, LegendResolved, LegendTheme) via with_attrs {
        /// Title above the entries (empty: no title row).
        title: RichText = |_| RichText::default(), LAYOUT;
        titlefont: Font = |_| Font::Bold, LAYOUT;
        titlesize: f64 = |g| g.fontsize, LAYOUT;
        titlecolor: Color = |g| g.textcolor, STYLE;
        titlehalign: HAlign = |_| HAlign::Center, LAYOUT;
        /// An invisible title still takes its space (Makie).
        titlevisible: bool = |_| true, STYLE;
        /// Gap between the title and the entries.
        titlegap: f64 = |_| 8.0, LAYOUT;
        labelsize: f64 = |g| g.fontsize, LAYOUT;
        labelfont: Font = |_| Font::Regular, LAYOUT;
        labelcolor: Color = |g| g.textcolor, STYLE;
        orientation: Orientation = |_| Orientation::Vertical, LAYOUT;
        /// Gap between entry rows.
        rowgap: f64 = |_| 3.0, LAYOUT;
        /// Gap between the label of one bank and the patch of the next.
        colgap: f64 = |_| 16.0, LAYOUT;
        /// Gap between each patch and its label.
        patchlabelgap: f64 = |_| 5.0, LAYOUT;
        /// Size of the box holding each entry's elements.
        patchsize: [f64; 2] = |_| [20.0, 20.0], LAYOUT;
        patchcolor: Color = |_| Color::TRANSPARENT, STYLE;
        patchstrokecolor: Color = |_| Color::TRANSPARENT, STYLE;
        patchstrokewidth: f64 = |_| 1.0, STYLE;
        /// Whether the frame and the background are drawn.
        framevisible: bool = |_| true, STYLE;
        framecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        framewidth: f64 = |_| 1.0, STYLE;
        backgroundcolor: Color = |_| Color::rgb(1.0, 1.0, 1.0), STYLE;
        /// Space between the frame and the content: one number or `(left, right, bottom, top)`.
        padding: [f64; 4] = |_| [6.0; 4], LAYOUT;
        /// Space around the frame (default 0; 6 for `axislegend`).
        margin: [f64; 4] = |_| [0.0; 4], LAYOUT;
        /// Alignment in the grid cell (or in the axis for `axislegend`).
        halign: HAlign = |_| HAlign::Center, LAYOUT;
        valign: VAlign = |_| VAlign::Center, LAYOUT;
        /// Fixed width (default: fit the content).
        width: Option<f64> = |_| None, LAYOUT;
        /// Fixed height (default: fit the content).
        height: Option<f64> = |_| None, LAYOUT;
        /// Whether the legend's width sizes its column (default: vertical legends do).
        tellwidth: bool = |_| true, LAYOUT;
        /// Whether the legend's height sizes its row (default: horizontal legends do).
        tellheight: bool = |_| false, LAYOUT;
    }
}

block_common!(Legend, Legend, LegendState);

impl Legend {
    fn with_attrs(&self, f: impl FnOnce(&mut LegendAttrs), dirty: u8) {
        self.with_state(dirty, |s| f(&mut s.attrs));
    }

    #[track_caller]
    fn create(pos: GridPosition, source: Source, check: &[&Arc<FigShared>]) -> Legend {
        let sh = pos.fig.sh.clone();
        for other in check {
            assert!(Arc::ptr_eq(&sh, other), "Legend: every axis and plot must belong to the legend's figure");
        }
        let state =
            LegendState { source, merge: false, unique: false, nbanks: 1, inside: None, attrs: LegendAttrs::default() };
        let id = sh.update(Dirty::LAYOUT, |st| {
            let place = pos.resolve(st);
            st.add_block(place, super::Block::Legend(Box::new(state)))
        });
        Legend { sh, id }
    }

    /// Makie's `Legend(fig[r, c], ax)` / `Legend(fig[r, c], [ax1, ax2])`: an entry for every
    /// labelled plot of the axes, in order. Entries are collected at every frame.
    #[track_caller]
    pub fn new(pos: GridPosition, src: impl LegendSource) -> Legend {
        let axes = src.__axes();
        let shs: Vec<&Arc<FigShared>> = axes.iter().map(|a| &a.sh).collect();
        Legend::create(pos, Source::Axes(axes.iter().map(|a| a.id).collect()), &shs)
    }

    /// Makie's `Legend(fig[r, c], plots, labels)`: explicit entries.
    ///
    /// ```
    /// use sciplot::prelude::*;
    /// let fig = Figure::new();
    /// let ax = Axis::new(fig.at(1, 1));
    /// let a = ax.lines([1.0, 2.0], [1.0, 2.0]);
    /// let b = ax.lines([1.0, 2.0], [2.0, 1.0]);
    /// Legend::from_entries(fig.at(1, 2), [(&a, "rising"), (&b, "falling")]);
    /// ```
    #[track_caller]
    pub fn from_entries<E: Into<LegendEntry>>(pos: GridPosition, entries: impl IntoIterator<Item = E>) -> Legend {
        let entries: Vec<LegendEntry> = entries.into_iter().map(Into::into).collect();
        let shs: Vec<&Arc<FigShared>> = entries.iter().flat_map(|e| e.plots.iter().map(|p| &p.sh)).collect();
        let stored = entries
            .iter()
            .map(|e| StoredEntry {
                label: e.label.clone(),
                plots: e.plots.iter().map(|p| p.id).collect(),
                elements: e.elements.clone(),
            })
            .collect();
        Legend::create(pos, Source::Entries(stored), &shs)
    }

    /// Layer all plots with the same label into one entry (Makie's `merge = true`).
    pub fn merge(&self, on: bool) -> Legend {
        self.with_state(Dirty::LAYOUT, |s| s.merge = on);
        self.clone()
    }

    /// Keep only the first plot of each (plot type, label) pair (Makie's `unique = true`).
    pub fn unique(&self, on: bool) -> Legend {
        self.with_state(Dirty::LAYOUT, |s| s.unique = on);
        self.clone()
    }

    /// Number of columns (vertical) or rows (horizontal) the entries are split into (default 1).
    pub fn nbanks(&self, n: usize) -> Legend {
        self.with_state(Dirty::LAYOUT, |s| s.nbanks = n.max(1));
        self.clone()
    }

    /// Sets `halign` and `valign` from an [`axislegend`] position.
    pub fn position(&self, p: Pos) -> Legend {
        let (h, v) = p.aligns();
        self.with_attrs(
            |a| {
                a.halign = Some(h);
                a.valign = Some(v);
            },
            Dirty::LAYOUT,
        );
        self.clone()
    }
}

/// Makie's `axislegend(ax)`: a legend of the axis' labelled plots drawn over its plot area,
/// at the top right by default, 6 units from the spines. Place it with `.position(Pos::LB)` etc.
///
/// ```
/// use sciplot::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// lines!(ax, [1.0, 2.0], [1.0, 2.0]; label = "a");
/// axislegend(&ax).position(Pos::LT);
/// axislegend!(ax; position = Pos::RB, title = "T");
/// ```
pub fn axislegend(ax: &crate::Axis) -> Legend {
    let axis_id = ax.id;
    let state = LegendState {
        source: Source::Axes(vec![axis_id]),
        merge: false,
        unique: false,
        nbanks: 1,
        inside: Some(axis_id),
        attrs: LegendAttrs { halign: Some(HAlign::Right), valign: Some(VAlign::Top), ..Default::default() },
    };
    let id = ax.sh.update(Dirty::LAYOUT, |st| {
        // Not part of the grid: reuse the axis' placement so the grid extent is unchanged.
        let place = st
            .iter_blocks()
            .find(|(id, _)| *id == axis_id)
            .map(|(_, s)| s.place.clone())
            .unwrap_or(crate::figure::Placement { rows: (1, 1), cols: (1, 1), side: crate::figure::Side::Inner });
        st.add_block(place, super::Block::Legend(Box::new(state)))
    });
    Legend { sh: ax.sh.clone(), id }
}

/// A resolved entry: label, elements, and whether all its plots are hidden.
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub label: RichText,
    pub elements: Vec<LegendElement>,
    pub hidden: bool,
}

/// Resolves plots to legend elements, caching each axis' cycle indices.
struct Elements<'a> {
    st: &'a FigState,
    g: &'a Globals,
    cycles: HashMap<BlockId, Vec<usize>>,
}

impl Elements<'_> {
    fn of(&mut self, pid: PlotId) -> Vec<LegendElement> {
        let st = self.st;
        let Some(p) = st.plot(pid) else { return Vec::new() };
        let Some(ax) = st.block(p.axis).and_then(|b| b.as_axis()) else { return Vec::new() };
        let cycles = self.cycles.entry(p.axis).or_insert_with(|| crate::scene::cycle_indices(st, &ax.plots));
        let cycle = ax.plots.iter().position(|q| *q == pid).and_then(|i| cycles.get(i).copied()).unwrap_or(0);
        p.kind.imp().legend_elements(&LegendCtx { theme: &st.theme, g: self.g, cycle })
    }
}

impl LegendState {
    fn resolve(&self, ctx: &BlockCtx<'_>) -> LegendResolved {
        let mut r = self.attrs.resolve(&ctx.st.theme.legend, ctx.g);
        if self.inside.is_some() && self.attrs.margin.is_none() && ctx.st.theme.legend.margin.is_none() {
            r.margin = [6.0; 4];
        }
        r
    }

    /// The current entries (Makie's `get_labeled_plots` for axis sources).
    pub(crate) fn entries(&self, st: &FigState, g: &Globals) -> Vec<Entry> {
        let mut el = Elements { st, g, cycles: HashMap::new() };
        let hidden =
            |plots: &[PlotId]| !plots.is_empty() && plots.iter().all(|p| st.plot(*p).is_none_or(|p| !p.common.visible));
        match &self.source {
            Source::Entries(es) => es
                .iter()
                .map(|e| {
                    let mut elements: Vec<LegendElement> = e.plots.iter().flat_map(|p| el.of(*p)).collect();
                    elements.extend(e.elements.iter().cloned());
                    Entry { label: e.label.clone(), elements, hidden: hidden(&e.plots) }
                })
                .collect(),
            Source::Axes(axes) => {
                let mut labelled: Vec<(PlotId, RichText)> = Vec::new();
                for a in axes {
                    let Some(ax) = st.block(*a).and_then(|b| b.as_axis()) else { continue };
                    for pid in &ax.plots {
                        let Some(p) = st.plot(*pid) else { continue };
                        let Some(label) = &p.common.label else { continue };
                        if self.unique {
                            let kind = std::mem::discriminant(&p.kind);
                            let dup = labelled.iter().any(|(q, l)| {
                                l == label && st.plot(*q).is_some_and(|q| std::mem::discriminant(&q.kind) == kind)
                            });
                            if dup {
                                continue;
                            }
                        }
                        labelled.push((*pid, label.clone()));
                    }
                }
                let mut groups: Vec<(RichText, Vec<PlotId>)> = Vec::new();
                for (pid, label) in labelled {
                    match groups.iter_mut().find(|(l, _)| self.merge && *l == label) {
                        Some((_, ps)) => ps.push(pid),
                        None => groups.push((label, vec![pid])),
                    }
                }
                groups
                    .into_iter()
                    .map(|(label, plots)| Entry {
                        label,
                        elements: plots.iter().flat_map(|p| el.of(*p)).collect(),
                        hidden: hidden(&plots),
                    })
                    .collect()
            }
        }
    }
}

/// One laid-out entry, relative to the content origin (top-left inside the padding).
#[derive(Clone, Debug)]
pub(crate) struct EntryGeom {
    pub patch: Rect,
    /// Patch plus label cell (the hidden-plot shade).
    pub cell: Rect,
    /// Left end of the label's vertical center.
    pub label_at: [f64; 2],
    pub label: TextLayout,
}

/// The legend's inner layout.
#[derive(Clone, Debug)]
pub(crate) struct Geometry {
    /// Content size (inside the padding).
    pub content: [f64; 2],
    /// Title layout and its anchor (horizontal position given by `titlehalign`, vertical center).
    pub title: Option<(TextLayout, [f64; 2])>,
    pub entries: Vec<EntryGeom>,
}

impl Geometry {
    /// Full size including padding and margin (the legend's autosize).
    pub fn outer(&self, r: &LegendResolved) -> [f64; 2] {
        let [pl, pr, pb, pt] = r.padding;
        let [ml, mr, mb, mt] = r.margin;
        [self.content[0] + pl + pr + ml + mr, self.content[1] + pb + pt + mb + mt]
    }
}

/// Makie's legend grid: title row, then `[patch | label]` column pairs.
pub(crate) fn geometry(r: &LegendResolved, nbanks: usize, entries: &[Entry]) -> Option<Geometry> {
    if entries.is_empty() {
        return None;
    }
    let nbanks = nbanks.max(1);
    let n = entries.len();
    // Entry k (0-based) -> (row, col); banks are columns when vertical, rows when horizontal.
    let rc = |k: usize| match r.orientation {
        Orientation::Vertical => (k / nbanks, k % nbanks),
        Orientation::Horizontal => (k % nbanks, k / nbanks),
    };
    let (nrows, ncols) = (0..n).map(rc).fold((0, 0), |(a, b), (i, j)| (a.max(i + 1), b.max(j + 1)));
    let labels: Vec<TextLayout> =
        entries.iter().map(|e| crate::text::layout(&e.label, r.labelsize, r.labelfont, r.labelcolor)).collect();
    let [pw, ph] = r.patchsize;
    let mut label_w = vec![0.0f64; ncols];
    let mut row_h = vec![ph; nrows];
    for (k, l) in labels.iter().enumerate() {
        let (i, j) = rc(k);
        label_w[j] = label_w[j].max(l.width);
        row_h[i] = row_h[i].max(l.height());
    }
    let mut col_x = Vec::with_capacity(ncols);
    let mut x = 0.0;
    for w in &label_w {
        col_x.push(x);
        x += pw + r.patchlabelgap + w + r.colgap;
    }
    let sub_w = x - r.colgap;
    let mut row_y = Vec::with_capacity(nrows);
    let mut y = 0.0;
    for h in &row_h {
        row_y.push(y);
        y += h + r.rowgap;
    }
    let sub_h = y - r.rowgap;

    let title = (!r.title.is_empty()).then(|| crate::text::layout(&r.title, r.titlesize, r.titlefont, r.titlecolor));
    let (tw, th) = title.as_ref().map_or((0.0, 0.0), |t| (t.width, t.height()));
    let width = sub_w.max(tw);
    let sub_top = if title.is_some() { th + r.titlegap } else { 0.0 };
    // The entry grid is centered under a wider title (Makie's `gridshalign = :center`).
    let sub_left = 0.5 * (width - sub_w);
    let entries = labels
        .into_iter()
        .enumerate()
        .map(|(k, label)| {
            let (i, j) = rc(k);
            let (cx, cy) = (sub_left + col_x[j], sub_top + row_y[i]);
            EntryGeom {
                patch: Rect::new(cx, cy + 0.5 * (row_h[i] - ph), pw, ph),
                cell: Rect::new(cx, cy, pw + r.patchlabelgap + label_w[j], row_h[i]),
                label_at: [cx + pw + r.patchlabelgap, cy + 0.5 * row_h[i]],
                label,
            }
        })
        .collect();
    let title = title.map(|t| {
        let h = r.titlehalign.frac();
        (t, [h * width, 0.5 * th])
    });
    Some(Geometry { content: [width, sub_top + sub_h], title, entries })
}

/// Legend draw order: above plots, axis decorations and their text.
mod z {
    pub const FRAME: f32 = 40.0;
    pub const PATCH: f32 = 41.0;
    pub const ELEMENTS: f32 = 42.0;
    pub const TEXT: f32 = 43.0;
    pub const SHADE: f32 = 44.0;
}

/// Rounds both corners to integers like Makie's `round_to_IRect2D`.
fn round_rect(r: Rect) -> Rect {
    let f = |v: f64| if v.is_finite() { v.round_ties_even() } else { 0.0 };
    let (x0, y0, x1, y1) = (f(r.x), f(r.y), f(r.right()), f(r.bottom()));
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// A rectangle outline of width `w` centered on the edges of `r` (Makie's poly stroke).
fn outline(r: Rect, w: f64, color: Color) -> [RectPrim; 4] {
    let h = 0.5 * w;
    let rp = |rect| RectPrim { rect, color, snap: true };
    [
        rp(Rect::new(r.x - h, r.y - h, r.w + w, w)),
        rp(Rect::new(r.x - h, r.bottom() - h, r.w + w, w)),
        rp(Rect::new(r.x - h, r.y + h, w, r.h - w)),
        rp(Rect::new(r.right() - h, r.y + h, w, r.h - w)),
    ]
}

fn quad(out: &mut Vec<MeshVertex>, r: Rect, color: Color) {
    let c = color.to_premul_u32();
    let v = |x: f64, y: f64| MeshVertex { pos: [x as f32, y as f32], color: c };
    let (a, b, cc, d) = (v(r.x, r.y), v(r.right(), r.y), v(r.x, r.bottom()), v(r.right(), r.bottom()));
    out.extend_from_slice(&[a, b, cc, b, d, cc]);
}

/// Draws one entry's elements into `patch` (figure units).
fn emit_elements(em: &mut Emitter, patch: Rect, elements: &[LegendElement]) {
    let center = [(patch.x + 0.5 * patch.w) as f32, (patch.y + 0.5 * patch.h) as f32];
    for e in elements {
        let prim = match e {
            LegendElement::Line { color, linewidth, linestyle } => Prim::Lines(LinesPrim {
                pts: Buf::transient(vec![[patch.x as f32, center[1]], [patch.right() as f32, center[1]]]),
                color: PrimColor::Uniform(*color),
                width: *linewidth as f32,
                pattern: linestyle.pattern(),
                cap: LineCap::Butt,
                join: JoinStyle::Miter,
                miter_limit: std::f32::consts::FRAC_PI_3,
                segments: false,
                closed: false,
                append: false,
            }),
            LegendElement::Marker { color, marker, markersize, strokecolor, strokewidth } => {
                Prim::Markers(MarkersPrim {
                    pos: Buf::transient(vec![center]),
                    color: PrimColor::Uniform(*color),
                    size: *markersize as f32,
                    sizes: None,
                    marker: *marker,
                    stroke_color: *strokecolor,
                    stroke_width: *strokewidth as f32,
                    rotation: 0.0,
                })
            }
            LegendElement::Poly { color, strokecolor, strokewidth } => {
                let mut v = Vec::with_capacity(6);
                quad(&mut v, patch, *color);
                em.push(z::ELEMENTS, None, Space::Figure, Prim::Mesh(MeshPrim { verts: Buf::transient(v) }));
                if *strokewidth > 0.0 && strokecolor.a > 0.0 {
                    let o = outline(patch, *strokewidth, *strokecolor).map(|mut r| {
                        r.snap = false;
                        r
                    });
                    em.push(z::ELEMENTS, None, Space::Figure, Prim::Rects(o.to_vec()));
                }
                continue;
            }
            LegendElement::Cells { colors } => {
                let (hw, hh) = (0.5 * patch.w, 0.5 * patch.h);
                let mut v = Vec::with_capacity(24);
                // [bottom-left, bottom-right, top-left, top-right] in a y-down figure.
                quad(&mut v, Rect::new(patch.x, patch.y + hh, hw, hh), colors[0]);
                quad(&mut v, Rect::new(patch.x + hw, patch.y + hh, hw, hh), colors[1]);
                quad(&mut v, Rect::new(patch.x, patch.y, hw, hh), colors[2]);
                quad(&mut v, Rect::new(patch.x + hw, patch.y, hw, hh), colors[3]);
                Prim::Mesh(MeshPrim { verts: Buf::transient(v) })
            }
        };
        em.push(z::ELEMENTS, None, Space::Figure, prim);
    }
}

impl BlockImpl for LegendState {
    fn layout(&self, ctx: &BlockCtx<'_>) -> BlockLayout {
        let r = self.resolve(ctx);
        let entries = self.entries(ctx.st, ctx.g);
        let size = geometry(&r, self.nbanks, &entries).map_or([0.0, 0.0], |g| g.outer(&r));
        let vertical = r.orientation == Orientation::Vertical;
        BlockLayout {
            protrusion: Protrusion::default(),
            width: r.width.map_or(BlockSize::Auto, BlockSize::Fixed),
            height: r.height.map_or(BlockSize::Auto, BlockSize::Fixed),
            autosize: [Some(size[0]), Some(size[1])],
            tellwidth: self.attrs.tellwidth.or(ctx.st.theme.legend.tellwidth).unwrap_or(vertical),
            tellheight: self.attrs.tellheight.or(ctx.st.theme.legend.tellheight).unwrap_or(!vertical),
            halign: r.halign.frac(),
            valign: r.valign.frac(),
            ..Default::default()
        }
    }

    fn emit(&self, ctx: &BlockCtx<'_>, em: &mut Emitter, rect: Rect) {
        let r = self.resolve(ctx);
        let entries = self.entries(ctx.st, ctx.g);
        let Some(geo) = geometry(&r, self.nbanks, &entries) else { return };
        let area = if self.inside.is_some() {
            // `rect` is the axis' plot area: align the legend in it (Makie's `bbox = viewport`).
            let [aw, ah] = geo.outer(&r);
            let (w, h) = (r.width.unwrap_or(aw), r.height.unwrap_or(ah));
            let (hf, vf) = (r.halign.frac(), r.valign.frac());
            Rect::new(rect.x + hf * (rect.w - w), rect.y + (1.0 - vf) * (rect.h - h), w, h)
        } else {
            rect
        };
        let area = round_rect(area);
        let [ml, mr, mb, mt] = r.margin;
        let lr = Rect::new(area.x + ml, area.y + mt, area.w - ml - mr, area.h - mt - mb);
        if r.framevisible {
            em.push(
                z::FRAME,
                None,
                Space::Figure,
                Prim::Rects(vec![RectPrim { rect: lr, color: r.backgroundcolor, snap: false }]),
            );
            if r.framewidth > 0.0 {
                em.push(z::FRAME, None, Space::Figure, Prim::Rects(outline(lr, r.framewidth, r.framecolor).to_vec()));
            }
        }
        let [pl, pr, pb, pt] = r.padding;
        // Content inside the padding; centered if the legend is larger than its content.
        let (cw, ch) = (lr.w - pl - pr, lr.h - pt - pb);
        let ox = lr.x + pl + 0.5 * (cw - geo.content[0]).max(0.0);
        let oy = lr.y + pt + 0.5 * (ch - geo.content[1]).max(0.0);
        let shift = |q: Rect| Rect::new(q.x + ox, q.y + oy, q.w, q.h);

        let mut glyphs = Vec::new();
        if let Some((t, at)) = &geo.title
            && r.titlevisible
        {
            let h = r.titlehalign.frac();
            glyphs.extend(crate::text::place(t, [ox + at[0], oy + at[1]], (h, 0.5), 0.0));
        }
        let mut patches = Vec::new();
        let mut shades = Vec::new();
        for (e, g) in entries.iter().zip(&geo.entries) {
            let patch = shift(g.patch);
            if r.patchcolor.a > 0.0 {
                patches.push(RectPrim { rect: patch, color: r.patchcolor, snap: false });
            }
            if r.patchstrokecolor.a > 0.0 && r.patchstrokewidth > 0.0 {
                patches.extend(outline(patch, r.patchstrokewidth, r.patchstrokecolor));
            }
            emit_elements(em, patch, &e.elements);
            glyphs.extend(crate::text::place(&g.label, [ox + g.label_at[0], oy + g.label_at[1]], (0.0, 0.5), 0.0));
            if e.hidden {
                // Makie shades entries whose plots are all hidden.
                shades.push(RectPrim { rect: shift(g.cell), color: Color::rgba(0.9, 0.9, 0.9, 0.65), snap: false });
            }
        }
        if !patches.is_empty() {
            em.push(z::PATCH, None, Space::Figure, Prim::Rects(patches));
        }
        if !glyphs.is_empty() {
            em.push(z::TEXT, None, Space::Figure, Prim::Glyphs(GlyphsPrim { glyphs }));
        }
        if !shades.is_empty() {
            em.push(z::SHADE, None, Space::Figure, Prim::Rects(shades));
        }
    }

    fn inside_axis(&self) -> Option<BlockId> {
        self.inside
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::scene::drawlist::DrawList;

    fn build(fig: &Figure) -> DrawList {
        crate::scene::build(&fig.sh.snapshot(), None, &mut crate::scene::SceneCache::new()).0
    }

    /// The legend frames' background rects (figure units, y down).
    fn frames(fig: &Figure) -> Vec<Rect> {
        build(fig)
            .items
            .iter()
            .filter(|it| it.z == z::FRAME)
            .filter_map(|it| match &it.prim {
                Prim::Rects(r) if r.len() == 1 => Some(r[0].rect),
                _ => None,
            })
            .collect()
    }

    fn legend_entries(leg: &Legend) -> Vec<Entry> {
        let st = leg.sh.snapshot();
        let g = st.theme.globals();
        match st.block(leg.id) {
            Some(crate::blocks::Block::Legend(l)) => l.entries(&st, &g),
            _ => panic!("no legend"),
        }
    }

    fn map(v: &[f64], f: impl Fn(f64) -> f64) -> Vec<f64> {
        v.iter().map(|x| f(*x)).collect()
    }

    // Expected frames come from tools/legend_check.jl (Makie's computed bbox, rounded like
    // `round_to_IRect2D`, minus the margin), for the figures of examples/legend_check.rs.

    #[test]
    fn axislegend_matches_makie() {
        let t = linspace(0.0, 10.0, 200);
        let fig = Figure::new();
        let ax = Axis!(fig.at(1, 1); xlabel = "t", ylabel = "u");
        lines!(ax, &t, map(&t, f64::sin); label = "sin");
        lines!(ax, &t, map(&t, f64::cos); label = "cos");
        axislegend!(ax);
        // Makie: bbox (513.22, 367, 70.78, 67) y-up, margin 6.
        assert_eq!(frames(&fig), vec![Rect::new(519.0, 22.0, 59.0, 55.0)]);
    }

    #[test]
    fn shared_unique_legend_matches_makie() {
        let t = linspace(0.0, 10.0, 200);
        let td = linspace(0.25, 9.75, 20);
        let fig = Figure!(size = (900, 650));
        let a = Axis!(fig.at(1, 1); title = "ω = 1", ylabel = "u (V)");
        let b = Axis!(fig.at(1, 2); title = "ω = 2");
        let c = Axis!(fig.at(2, 1..=2); xlabel = "t (s)");
        for (k, ax) in [&a, &b, &c].into_iter().enumerate() {
            let k = (k + 1) as f64;
            lines!(ax, &t, map(&t, |x| (k * x).sin()); label = "model");
            scatter!(ax, &td, map(&td, |x| (k * x).sin() + 0.1 * (7.0 * x).cos()); label = "measurement", markersize = 7);
        }
        lines!(c, &t, map(&t, |x| 0.5 * x.cos()); label = "envelope", linestyle = Linestyle::Dash);
        let leg = Legend!(fig.at(1..=2, 3), &[&a, &b, &c]; unique = true);
        let labels: Vec<String> = legend_entries(&leg).iter().map(|e| e.label.plain_text()).collect();
        assert_eq!(labels, ["model", "measurement", "envelope"]);
        // Makie: bbox (761.42, 297.16, 122.58, 78) y-up in a 650-unit-high figure.
        assert_eq!(frames(&fig), vec![Rect::new(761.0, 275.0, 123.0, 78.0)]);
        // The envelope is the second `lines` of axis c: orange.
        let e = &legend_entries(&leg)[2].elements[0];
        assert!(matches!(e, LegendElement::Line { color, .. } if *color == WONG[1]));
    }

    #[test]
    fn horizontal_legend_with_title_matches_makie() {
        let t = linspace(0.0, 10.0, 200);
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        lines!(ax, &t, map(&t, f64::sin); label = "solid");
        lines!(ax, &t, map(&t, |x| (x - 1.0).sin()); label = "dash", linestyle = Linestyle::Dash);
        lines!(ax, &t, map(&t, |x| (x - 2.0).sin()); label = "dot", linestyle = Linestyle::Dot, linewidth = 3);
        let k: Vec<f64> = (1..=9).map(f64::from).collect();
        scatterlines!(ax, &k, map(&k, |x| 0.2 * x.cos()); label = "scatterlines");
        Legend!(fig.at(2, 1), &ax; title = "Styles", orientation = Orientation::Horizontal);
        // Makie: bbox (163.62, 16, 309.39, 56.31) y-up.
        assert_eq!(frames(&fig), vec![Rect::new(164.0, 378.0, 309.0, 56.0)]);
    }

    #[test]
    fn titled_axislegend_left_top_matches_makie() {
        let t = linspace(0.0, 10.0, 200);
        let k: Vec<f64> = (1..=9).map(f64::from).collect();
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        band!(ax, &t, map(&t, |x| x.sin() - 0.3 + 3.0), map(&t, |x| x.sin() + 0.3 + 3.0); label = "band");
        barplot!(ax, &k, map(&k, |x| 1.0 + 0.1 * x); label = "bars");
        barplot!(ax, &k, map(&k, |x| 0.5 + 0.05 * x); label = "bars 2", strokewidth = 1, strokecolor = BLACK);
        scatter!(ax, &k, map(&k, |x| 2.0 + 0.1 * x);
            label = "stroked", marker = Marker::Rect, markersize = 12, strokewidth = 1, color = ORANGE);
        let leg = axislegend!(ax; title = "Kinds", position = Pos::LT);
        // Makie: bbox (33, 296.69, 94.91, 137.31) y-up, margin 6.
        assert_eq!(frames(&fig), vec![Rect::new(39.0, 22.0, 83.0, 125.0)]);
        let es = legend_entries(&leg);
        let pal = crate::theme::Globals::default().patchpalette;
        assert_eq!(
            es[0].elements,
            [LegendElement::Poly { color: pal[0], strokecolor: Color::TRANSPARENT, strokewidth: 0.0 }]
        );
        assert!(matches!(es[2].elements[0], LegendElement::Poly { color, strokewidth: 1.0, .. } if color == pal[1]));
        assert!(matches!(es[3].elements[0], LegendElement::Marker { marker: Marker::Rect, markersize: 12.0, .. }));
    }

    #[test]
    fn merge_unique_and_late_labels() {
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        let leg = Legend::new(fig.at(1, 2), &ax);
        assert!(legend_entries(&leg).is_empty());
        assert!(frames(&fig).is_empty(), "an empty legend draws nothing");
        let l1 = ax.lines([1.0, 2.0], [1.0, 2.0]).label("a");
        ax.scatter([1.0, 2.0], [1.0, 2.0]).label("a");
        ax.lines([1.0, 2.0], [2.0, 1.0]).label("a");
        let unlabeled = ax.lines([1.0, 2.0], [0.0, 0.0]);
        assert_eq!(legend_entries(&leg).len(), 3, "plots labelled after the legend appear");
        assert_eq!(frames(&fig).len(), 1);
        leg.unique(true);
        assert_eq!(legend_entries(&leg).len(), 2, "unique keeps one entry per (type, label)");
        leg.unique(false).merge(true);
        let es = legend_entries(&leg);
        assert_eq!(es.len(), 1);
        assert!(matches!(
            es[0].elements.as_slice(),
            [LegendElement::Line { .. }, LegendElement::Marker { .. }, LegendElement::Line { .. }]
        ));
        unlabeled.label("late");
        assert_eq!(legend_entries(&leg).len(), 2);
        // Entries whose plots are all hidden are shaded.
        l1.visible(false);
        assert!(!legend_entries(&leg)[0].hidden, "only some of the merged plots are hidden");
        leg.merge(false);
        assert!(legend_entries(&leg)[0].hidden);
        assert!(build(&fig).items.iter().any(|it| it.z == z::SHADE));
    }

    #[test]
    fn colors_follow_the_plot_cycle() {
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        ax.lines([1.0, 2.0], [1.0, 2.0]).label("1");
        ax.lines([1.0, 2.0], [1.0, 2.0]).color(RED).label("red");
        ax.lines([1.0, 2.0], [1.0, 2.0]).label("2");
        ax.lines([1.0, 2.0], [1.0, 2.0]).color(Cycled(3)).label("c3");
        ax.lines([1.0, 2.0], [1.0, 2.0]).color(vec![1.0, 2.0]).label("values");
        ax.scatter([1.0], [1.0]).label("s").alpha(0.5);
        ax.scatterlines([1.0], [1.0]).markercolor(BLUE).label("sl");
        ax.heatmap(Field::new(&[1.0, 2.0, 3.0, 4.0], 2, 2)).label("hm");
        ax.text([1.0], [1.0], ["t"]).label("text");
        let leg = Legend::new(fig.at(1, 2), &ax);
        let color = |e: &LegendElement| match e {
            LegendElement::Line { color, .. } | LegendElement::Marker { color, .. } => *color,
            other => panic!("{other:?}"),
        };
        let es = legend_entries(&leg);
        let c: Vec<Color> = es[..6].iter().map(|e| color(&e.elements[0])).collect();
        assert_eq!(c, [WONG[0], RED, WONG[1], WONG[2], Color::rgb(0.0, 0.0, 0.0), WONG[0].with_alpha(0.5)]);
        assert_eq!(color(&es[6].elements[0]), WONG[0]);
        assert_eq!(color(&es[6].elements[1]), BLUE);
        let viridis = Colormap::VIRIDIS;
        assert_eq!(
            es[7].elements,
            [LegendElement::Cells {
                colors: [viridis.sample(0.0), viridis.sample(0.6), viridis.sample(0.3), viridis.sample(1.0)]
            }]
        );
        assert!(es[8].elements.is_empty(), "text has no legend element but keeps its entry");
    }

    #[test]
    fn explicit_entries_and_theme() {
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        let a = ax.lines([1.0, 2.0], [1.0, 2.0]);
        let s = ax.scatter([1.0, 2.0], [1.0, 2.0]);
        let leg = Legend::from_entries(
            fig.at(1, 2),
            [
                LegendEntry::new("a", &a),
                (PlotRef::from(&s), "s").into(),
                LegendEntry::merged("both", [PlotRef::from(&a), PlotRef::from(&s)]),
                LegendEntry::elements(
                    "custom",
                    [LegendElement::Poly { color: RED, strokecolor: BLACK, strokewidth: 1.0 }],
                ),
            ],
        );
        let es = legend_entries(&leg);
        assert_eq!(es.iter().map(|e| e.elements.len()).collect::<Vec<_>>(), [1, 1, 2, 1]);
        // A vertical legend tells its width; its size is the entry grid plus padding.
        let st = fig.sh.snapshot();
        let g = st.theme.globals();
        let Some(crate::blocks::Block::Legend(l)) = st.block(leg.id) else { panic!("no legend") };
        let bl = l.layout(&BlockCtx { st: &st, g: &g, axes: &[], id: leg.id });
        assert!(bl.tellwidth && !bl.tellheight);
        let w = crate::text::measure(&"custom".into(), 14.0, Font::Regular).width;
        assert!((bl.autosize[0].unwrap() - (12.0 + 20.0 + 5.0 + w)).abs() < 1e-9);
        assert!((bl.autosize[1].unwrap() - (12.0 + 4.0 * 20.0 + 3.0 * 3.0)).abs() < 1e-9);

        // theme_minimal hides the frame (and with it the background).
        let fig = with_theme(theme_minimal(), Figure::new);
        let ax = Axis::new(fig.at(1, 1));
        ax.lines([1.0, 2.0], [1.0, 2.0]).label("a");
        axislegend(&ax);
        assert!(frames(&fig).is_empty());
        assert!(build(&fig).items.iter().any(|it| it.z == z::TEXT));
    }

    #[test]
    fn plot_ref_is_shared_with_colorbars() {
        // One `PlotRef` type for legend entries and colormapped plots; every plot handle converts.
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        let h = ax.hlines([0.5]);
        let sl = ax.scatterlines([1.0, 2.0], [1.0, 2.0]);
        let refs = [PlotRef::from(&h), PlotRef::from(h.clone()), PlotRef::from(&sl)];
        assert_eq!(refs[0], refs[1]);
        assert_ne!(refs[0], refs[2]);
        assert_eq!(crate::plots::ColorMapped::plot_ref(&sl), refs[2]);
        let leg = Legend::from_entries(fig.at(1, 2), [LegendEntry::new("h", &h), LegendEntry::merged("both", refs)]);
        // Reference lines have no legend element yet; scatterlines has a line and a marker.
        assert_eq!(legend_entries(&leg).iter().map(|e| e.elements.len()).collect::<Vec<_>>(), [0, 2]);
    }
}
