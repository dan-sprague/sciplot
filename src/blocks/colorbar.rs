//! `Colorbar`: a color scale next to a plot (Makie's `Colorbar`).
//!
//! Geometry follows Makie's `colorbar.jl` and `LineAxis`: the bar fills its cell along its long
//! side and is `size` units thick; ticks, tick labels and the label sit on the `flipaxis` side
//! and are reported as protrusions, so a colorbar next to an axis lines up with its spines. The
//! gradient is `nsteps - 1` interpolated cells (Makie's image of midpoints), and clip colors set
//! on the plot add triangles at the bar's ends.

use super::{BlockCtx, BlockImpl, BlockLayout, block_common};
use crate::attrs::{Conv, attributes};
use crate::color::{Color, Colormap};
use crate::figure::{BlockId, Dirty, FigShared, GridPosition, PlotId};
use crate::layout::{BlockSize, Protrusion};
use crate::plots::{ColorMapped, ResolvedColormap};
use crate::scene::axis::z;
use crate::scene::drawlist::{
    Buf, ColorMapping, Emitter, FieldPrim, GlyphsPrim, GridAxis, LinesPrim, MeshPrim, MeshVertex, Prim, PrimColor,
    Rect, RectPrim, Space,
};
use crate::style::{HAlign, JoinStyle, LineCap, VAlign};
use crate::text::{Font, RichText};
use crate::ticks::{MinorSpec, TickFormat, TickSpec, Ticks};
use crate::transform::Scale;
use std::f64::consts::FRAC_PI_2;
use std::sync::Arc;

/// A colorbar block (Makie's `Colorbar`).
///
/// Linked to a plot, it shows the plot's colormap, colorrange, clip colors and alpha as they are
/// on every frame, so live updates (new data, a new `colorrange`) propagate. Without a plot it
/// shows a colormap over fixed limits.
///
/// ```
/// use sciplot::prelude::*;
/// let z: Vec<f64> = (0..200).map(|k| (k as f64 * 0.1).sin()).collect();
/// let fig = Figure::new();
/// let hm = Axis::new(fig.at(1, 1)).heatmap(Field::new(&z, 20, 10)).colormap(Colormap::MAGMA);
/// Colorbar::new(fig.at(1, 2), &hm).label("amplitude");
/// Colorbar::from_colormap(fig.at(2, 1), Colormap::VIRIDIS, (0, 10)).vertical(false).flipaxis(false);
/// ```
#[derive(Clone)]
pub struct Colorbar {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: BlockId,
}

/// Where the colorbar's color mapping comes from.
#[derive(Clone, Debug)]
pub(crate) enum Source {
    /// A colormapped plot (read at every frame).
    Plot(PlotId),
    /// The colorbar's own `colormap`, `limits`, `lowclip` and `highclip`.
    Own,
}

#[derive(Clone, Debug)]
pub(crate) struct ColorbarState {
    pub source: Source,
    /// Number of samples of the colorrange (Makie's `nsteps`).
    pub nsteps: usize,
    pub attrs: ColorbarAttrs,
}

attributes! {
    Colorbar(ColorbarAttrs, ColorbarResolved, ColorbarTheme) via with_attrs {
        /// The label next to the tick labels.
        label: RichText = |_| RichText::default(), LAYOUT;
        labelcolor: Color = |g| g.textcolor, STYLE;
        labelfont: Font = |_| Font::Regular, LAYOUT;
        labelsize: f64 = |g| g.fontsize, LAYOUT;
        labelvisible: bool = |_| true, LAYOUT;
        /// Gap between the tick labels and the label.
        labelpadding: f64 = |_| 5.0, LAYOUT;
        /// Label rotation in radians; `None` (automatic) reads bottom to top on vertical bars.
        labelrotation: Option<f64> = |_| None, LAYOUT;
        /// Rotate an automatic vertical label to read top to bottom.
        flip_vertical_label: bool = |_| false, LAYOUT;
        ticklabelfont: Font = |_| Font::Regular, LAYOUT;
        ticklabelsize: f64 = |g| g.fontsize, LAYOUT;
        ticklabelsvisible: bool = |_| true, LAYOUT;
        ticklabelcolor: Color = |g| g.textcolor, STYLE;
        /// Fixed room for the tick labels (units); `None` fits the current labels.
        ticklabelspace: Option<f64> = |_| None, LAYOUT;
        /// Gap between the tick marks and the tick labels.
        ticklabelpad: f64 = |_| 3.0, LAYOUT;
        /// Major ticks: `TickSpec::Automatic` (Wilkinson on the colorrange), values, `(values,
        /// labels)` or a function of the limits.
        ticks: TickSpec = |_| TickSpec::Automatic, LAYOUT;
        /// Tick label format: automatic (Makie's), a closure `|v: f64| String`, or a format string.
        tickformat: TickFormat = |_| TickFormat::Automatic, LAYOUT;
        ticksize: f64 = |_| 5.0, LAYOUT;
        ticksvisible: bool = |_| true, LAYOUT;
        /// 0 = outward, 1 = inward.
        tickalign: f64 = |_| 0.0, LAYOUT;
        tickwidth: f64 = |_| 1.0, STYLE;
        tickcolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        minorticksvisible: bool = |_| false, STYLE;
        /// Minor ticks (Makie default `IntervalsBetween(5)`).
        minorticks: MinorSpec = |_| MinorSpec::IntervalsBetween(5), STYLE;
        minorticksize: f64 = |_| 3.0, STYLE;
        minortickwidth: f64 = |_| 1.0, STYLE;
        minortickalign: f64 = |_| 0.0, STYLE;
        minortickcolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        /// Width of the frame around the bar.
        spinewidth: f64 = |_| 1.0, STYLE;
        /// Color of the frame around the bar (Makie's `topspinecolor`, which the frame uses).
        spinecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        /// Vertical bar (default) or horizontal.
        vertical: bool = |_| true, LAYOUT;
        /// Ticks and label on the right (vertical) or top (horizontal); `false`: left or bottom.
        flipaxis: bool = |_| true, LAYOUT;
        /// Thickness of the bar in units (Makie default 12).
        size: f64 = |_| 12.0, LAYOUT;
        /// Fixed width in units (`None`: `size` when vertical, fill the cell when horizontal).
        width: Option<f64> = |_| None, LAYOUT;
        /// Fixed height in units (`None`: fill the cell when vertical, `size` when horizontal).
        height: Option<f64> = |_| None, LAYOUT;
        halign: HAlign = |_| HAlign::Center, LAYOUT;
        valign: VAlign = |_| VAlign::Center, LAYOUT;
        tellwidth: bool = |_| true, LAYOUT;
        tellheight: bool = |_| true, LAYOUT;
        /// Colormap of a colorbar without a plot (default viridis).
        colormap: Colormap = |_| Colormap::VIRIDIS, LAYOUT;
        /// `(lo, hi)` of a colorbar without a plot (default `(0, 1)`); alias `colorrange`.
        limits: Option<[f64; 2]> = |_| None, LAYOUT;
        /// Low clip triangle color of a colorbar without a plot (default: no triangle).
        lowclip: Option<Color> = |_| None, LAYOUT;
        /// High clip triangle color of a colorbar without a plot (default: no triangle).
        highclip: Option<Color> = |_| None, LAYOUT;
    }
}

block_common!(Colorbar, Colorbar, ColorbarState);

impl Colorbar {
    fn with_attrs(&self, f: impl FnOnce(&mut ColorbarAttrs), dirty: u8) {
        self.with_state(dirty, |s| {
            f(&mut s.attrs);
            if matches!(s.source, Source::Plot(_)) {
                let a = &mut s.attrs;
                if a.colormap.is_some() || a.limits.is_some() || a.lowclip.is_some() || a.highclip.is_some() {
                    crate::warn_once(
                        "Colorbar: colormap, limits, lowclip and highclip come from the plot; set them on the plot instead",
                    );
                    (a.colormap, a.limits, a.lowclip, a.highclip) = (None, None, None, None);
                }
            }
        });
    }

    fn create(pos: GridPosition, source: Source, attrs: ColorbarAttrs) -> Colorbar {
        let sh = pos.fig.sh.clone();
        let id = sh.update(Dirty::LAYOUT, |st| {
            let place = pos.resolve(st);
            st.add_block(place, super::Block::Colorbar(Box::new(ColorbarState { source, nsteps: 100, attrs })))
        });
        Colorbar { sh, id }
    }

    /// Makie's `Colorbar(fig[r, c], plot)`: a colorbar showing `plot`'s colormap, colorrange,
    /// `lowclip`/`highclip` (as triangles, when set) and alpha, following their changes.
    ///
    /// A plot whose colors are not values mapped through its colormap (a solid or per-point
    /// color) shows its colormap over `(0, 1)`, with a warning.
    ///
    /// # Panics
    /// If `plot` belongs to another figure.
    #[track_caller]
    pub fn new(pos: GridPosition, plot: &impl ColorMapped) -> Colorbar {
        let p = plot.plot_ref();
        assert!(Arc::ptr_eq(&p.sh, &pos.fig.sh), "Colorbar::new: the plot belongs to another figure");
        if plot.colormapping().is_some_and(|m| !m.mapped) {
            warn_unmapped();
        }
        Colorbar::create(pos, Source::Plot(p.id), ColorbarAttrs::default())
    }

    /// Makie's `Colorbar(fig[r, c]; colormap, limits)`: a colorbar for a colormap over
    /// `(lo, hi)`, without a plot.
    #[track_caller]
    pub fn from_colormap(
        pos: GridPosition,
        colormap: impl crate::color::IntoColormap,
        limits: impl Conv<[f64; 2]>,
    ) -> Colorbar {
        let attrs = ColorbarAttrs {
            colormap: Some(colormap.into_colormap()),
            limits: Some(Some(limits.conv())),
            ..Default::default()
        };
        Colorbar::create(pos, Source::Own, attrs)
    }

    /// Makie's `Colorbar(fig[r, c])`: a colorbar without a plot, showing the theme's colormap
    /// (viridis) over `(0, 1)` until `.colormap(..)` / `.limits(..)` are set.
    pub fn standalone(pos: GridPosition) -> Colorbar {
        Colorbar::create(pos, Source::Own, ColorbarAttrs::default())
    }

    /// Alias of [`limits`](Colorbar::limits) (Makie accepts both).
    #[track_caller]
    pub fn colorrange(&self, v: impl Conv<Option<[f64; 2]>>) -> Colorbar {
        self.limits(v)
    }

    /// Number of samples of the colorrange in the gradient (Makie's `nsteps`, default 100).
    pub fn nsteps(&self, n: usize) -> Colorbar {
        self.with_state(Dirty::STYLE, |s| s.nsteps = n.max(2));
        self.clone()
    }

    /// The color mapping the colorbar shows right now (the linked plot's, or its own).
    pub fn colormapping(&self) -> Option<ResolvedColormap> {
        let st = self.sh.state.lock();
        let g = st.theme.globals();
        match st.block(self.id) {
            Some(super::Block::Colorbar(b)) => Some(b.mapping(&st, &g).0),
            _ => None,
        }
    }
}

fn warn_unmapped() {
    crate::warn_once(
        "Colorbar: the plot's colors are not values mapped through its colormap; showing its colormap over (0, 1)",
    );
}

/// Everything a frame of the colorbar needs.
struct Frame {
    r: ColorbarResolved,
    map: ResolvedColormap,
    lo: f64,
    hi: f64,
    ticks: Ticks,
    nsteps: usize,
}

/// Width and height of text rotated by `angle`.
fn rotated_extent(w: f64, h: f64, angle: f64) -> (f64, f64) {
    let (s, c) = angle.sin_cos();
    (w * c.abs() + h * s.abs(), w * s.abs() + h * c.abs())
}

impl ColorbarState {
    /// The mapping to show: the plot's (if it still exists) or the colorbar's own attributes.
    /// The flag is `false` when a linked plot was deleted.
    fn mapping(&self, st: &crate::figure::FigState, g: &crate::theme::Globals) -> (ResolvedColormap, bool) {
        let r = self.attrs.resolve(&st.theme.colorbar, g);
        let own = || {
            let [lo, hi] = r.limits.unwrap_or([0.0, 1.0]);
            ResolvedColormap {
                colormap: r.colormap.clone(),
                colorrange: (lo, hi),
                lowclip: r.lowclip,
                highclip: r.highclip,
                alpha: 1.0,
                mapped: true,
            }
        };
        match self.source {
            Source::Own => (own(), true),
            Source::Plot(id) => match st.plot(id).and_then(|p| p.kind.imp().colormapping(&st.theme, g)) {
                Some(m) => (m, true),
                None => (own(), false),
            },
        }
    }

    fn frame(&self, ctx: &BlockCtx<'_>) -> Frame {
        let r = self.attrs.resolve(&ctx.st.theme.colorbar, ctx.g);
        let (map, alive) = self.mapping(ctx.st, ctx.g);
        if !alive {
            crate::warn_once("Colorbar: its plot was deleted; showing the colorbar's own colormap");
        } else if !map.mapped {
            warn_unmapped();
        }
        let (mut lo, mut hi) = map.colorrange;
        if !(lo.is_finite() && hi.is_finite()) {
            (lo, hi) = (0.0, 1.0);
        } else if lo == hi {
            (lo, hi) = (lo - 0.5, hi + 0.5);
        }
        let ticks = crate::ticks::resolve_ticks(&r.ticks, &r.tickformat, lo, hi, Scale::Identity);
        Frame { r, map, lo, hi, ticks, nsteps: self.nsteps.max(2) }
    }
}

impl Frame {
    /// Makie's `LineAxis` tick space: how far ticks stick out of the frame.
    fn tickspace(&self) -> f64 {
        if self.r.ticksvisible { (self.r.ticksize * (1.0 - self.r.tickalign)).max(0.0) } else { 0.0 }
    }

    /// Room for the tick labels across the bar (`actual_ticklabelspace`).
    fn ticklabelspace(&self) -> f64 {
        let r = &self.r;
        if let Some(s) = r.ticklabelspace {
            return s;
        }
        if !r.ticklabelsvisible {
            return 0.0;
        }
        self.ticks
            .labels
            .iter()
            .map(|l| {
                let t = crate::text::layout(l, r.ticklabelsize, r.ticklabelfont, r.ticklabelcolor);
                if r.vertical { t.width } else { t.height() }
            })
            .fold(0.0, f64::max)
    }

    /// Label rotation and alignment (Makie's automatic `labelrotation` / `labelalign`).
    fn label_rotation(&self) -> f64 {
        match self.r.labelrotation {
            Some(a) => a,
            None if self.r.vertical => {
                if self.r.flip_vertical_label {
                    -FRAC_PI_2
                } else {
                    FRAC_PI_2
                }
            }
            None => 0.0,
        }
    }

    fn label_visible(&self) -> bool {
        self.r.labelvisible && !self.r.label.plain_text().trim().is_empty()
    }

    /// The label's extent across the bar.
    fn label_extent(&self) -> f64 {
        let r = &self.r;
        let l = crate::text::layout(&r.label, r.labelsize, r.labelfont, r.labelcolor);
        let (w, h) = rotated_extent(l.width, l.height(), self.label_rotation());
        if r.vertical { w } else { h }
    }

    /// Makie's `calculate_protrusion` for the colorbar's `LineAxis` (the spine width is not
    /// counted, as in Makie).
    fn protrusion(&self) -> f64 {
        let r = &self.r;
        let labelspace = if self.label_visible() { self.label_extent() + r.labelpadding } else { 0.0 };
        let tickspace = if r.ticksvisible && !self.ticks.labels.is_empty() { self.tickspace() } else { 0.0 };
        let space = self.ticklabelspace();
        let ticklabelgap = if r.ticklabelsvisible && space > 0.0 { space + r.ticklabelpad } else { 0.0 };
        tickspace + ticklabelgap + labelspace
    }
}

impl BlockImpl for ColorbarState {
    fn layout(&self, ctx: &BlockCtx<'_>) -> BlockLayout {
        let f = self.frame(ctx);
        let r = &f.r;
        let p = f.protrusion();
        let protrusion = match (r.vertical, r.flipaxis) {
            (true, true) => Protrusion { right: p, ..Default::default() },
            (true, false) => Protrusion { left: p, ..Default::default() },
            (false, true) => Protrusion { top: p, ..Default::default() },
            (false, false) => Protrusion { bottom: p, ..Default::default() },
        };
        let size = |v: Option<f64>| v.map_or(BlockSize::Auto, BlockSize::Fixed);
        BlockLayout {
            protrusion,
            width: size(r.width),
            height: size(r.height),
            autosize: if r.vertical { [Some(r.size), None] } else { [None, Some(r.size)] },
            tellwidth: r.tellwidth,
            tellheight: r.tellheight,
            halign: r.halign.frac(),
            valign: r.valign.frac(),
            ..Default::default()
        }
    }

    fn emit(&self, ctx: &BlockCtx<'_>, em: &mut Emitter, rect: Rect) {
        let f = self.frame(ctx);
        emit_colorbar(&f, em, rect);
    }
}

/// Premultiplied triangle vertices.
fn triangle(pts: [[f64; 2]; 3], c: Color) -> [MeshVertex; 3] {
    let color = c.to_premul_u32();
    pts.map(|p| MeshVertex { pos: [p[0] as f32, p[1] as f32], color })
}

fn emit_colorbar(f: &Frame, em: &mut Emitter, rect: Rect) {
    let r = &f.r;
    let v = r.vertical;
    // Makie's `round_to_IRect2D` of the layout box.
    let round = |x: f64| x.round_ties_even();
    let (x0, x1, y0, y1) = (round(rect.x), round(rect.right()), round(rect.y), round(rect.bottom()));
    if !(x1 > x0 && y1 > y0) {
        return;
    }
    // Clip triangles: equilateral on the bar's end (height thickness · sin 60°). The bar shortens
    // by the full thickness, as in Makie (its `tri_heights` returns before applying the sin 60°
    // factor), so the apex stops short of the layout box by thickness · (1 - sin 60°).
    let thick = if v { x1 - x0 } else { y1 - y0 };
    let tri = thick * (std::f64::consts::PI / 3.0).sin();
    let lo_h = if f.map.lowclip.is_some() { thick } else { 0.0 };
    let hi_h = if f.map.highclip.is_some() { thick } else { 0.0 };
    let bar = if v {
        Rect::new(x0, y0 + hi_h, x1 - x0, (y1 - lo_h) - (y0 + hi_h))
    } else {
        Rect::new(x0 + lo_h, y0, (x1 - hi_h) - (x0 + lo_h), y1 - y0)
    };
    if !(bar.w > 0.0 && bar.h > 0.0) {
        return;
    }

    // Gradient: Makie's image of the `nsteps - 1` midpoints of `LinRange(lo, hi, nsteps)`,
    // interpolated. Values are stored normalized to 0..1.
    let n = f.nsteps - 1;
    let values: Vec<f32> = (0..n).map(|k| ((k as f64 + 0.5) / n as f64) as f32).collect();
    let (nx, ny) = if v { (1, n as u32) } else { (n as u32, 1) };
    em.push(
        0.0,
        None,
        Space::Figure,
        Prim::Field(FieldPrim {
            values: Buf::transient(values),
            nx,
            ny,
            x: GridAxis::Regular { e0: bar.x, e1: bar.right() },
            y: GridAxis::Regular { e0: bar.bottom(), e1: bar.y },
            map: ColorMapping {
                lut: f.map.colormap.lut(),
                range: [0.0, 1.0],
                lowclip: None,
                highclip: None,
                nan_color: Color::TRANSPARENT,
                alpha: f.map.alpha as f32,
            },
            interpolate: true,
        }),
    );

    // Clip triangles and their apexes (for the frame).
    let mid = if v { 0.5 * (bar.x + bar.right()) } else { 0.5 * (bar.y + bar.bottom()) };
    let hi_apex = if v { [mid, bar.y - tri] } else { [bar.right() + tri, mid] };
    let lo_apex = if v { [mid, bar.bottom() + tri] } else { [bar.x - tri, mid] };
    let mut verts = Vec::new();
    if let Some(c) = f.map.highclip {
        let base = if v {
            [[bar.x, bar.y], [bar.right(), bar.y]]
        } else {
            [[bar.right(), bar.y], [bar.right(), bar.bottom()]]
        };
        verts.extend(triangle([base[0], base[1], hi_apex], c));
    }
    if let Some(c) = f.map.lowclip {
        let base = if v {
            [[bar.x, bar.bottom()], [bar.right(), bar.bottom()]]
        } else {
            [[bar.x, bar.y], [bar.x, bar.bottom()]]
        };
        verts.extend(triangle([base[0], base[1], lo_apex], c));
    }
    if !verts.is_empty() {
        em.push(0.0, None, Space::Figure, Prim::Mesh(MeshPrim { verts: Buf::transient(verts) }));
    }

    // Frame: crisp rectangles like the Axis spines, or Makie's closed polyline around the bar and
    // its triangles.
    let sw = r.spinewidth;
    if sw > 0.0 {
        if f.map.lowclip.is_none() && f.map.highclip.is_none() {
            let rects = [
                Rect::new(bar.x - 0.5 * sw, bar.y - 0.5 * sw, sw, bar.h + sw),
                Rect::new(bar.right() - 0.5 * sw, bar.y - 0.5 * sw, sw, bar.h + sw),
                Rect::new(bar.x - 0.5 * sw, bar.y - 0.5 * sw, bar.w + sw, sw),
                Rect::new(bar.x - 0.5 * sw, bar.bottom() - 0.5 * sw, bar.w + sw, sw),
            ];
            let rects = rects.iter().map(|&rect| RectPrim { rect, color: r.spinecolor, snap: true }).collect();
            em.push(z::SPINES, None, Space::Figure, Prim::Rects(rects));
        } else {
            let (bl, br, tl, tr) =
                ([bar.x, bar.bottom()], [bar.right(), bar.bottom()], [bar.x, bar.y], [bar.right(), bar.y]);
            let mut pts = Vec::with_capacity(7);
            if v {
                pts.extend([br, tr]);
                if f.map.highclip.is_some() {
                    pts.push(hi_apex);
                }
                pts.extend([tl, bl]);
                if f.map.lowclip.is_some() {
                    pts.push(lo_apex);
                }
                pts.push(br);
            } else {
                pts.extend([bl, br]);
                if f.map.highclip.is_some() {
                    pts.push(hi_apex);
                }
                pts.extend([tr, tl]);
                if f.map.lowclip.is_some() {
                    pts.push(lo_apex);
                }
                pts.push(bl);
            }
            let pts: Vec<[f32; 2]> = pts.iter().map(|p| [p[0] as f32, p[1] as f32]).collect();
            em.push(
                z::SPINES,
                None,
                Space::Figure,
                Prim::Lines(LinesPrim {
                    pts: Buf::transient(pts),
                    color: PrimColor::Uniform(r.spinecolor),
                    width: sw as f32,
                    pattern: None,
                    cap: LineCap::Butt,
                    join: JoinStyle::Miter,
                    miter_limit: std::f32::consts::FRAC_PI_3,
                    segments: false,
                    closed: true,
                    append: false,
                }),
            );
        }
    }

    // The axis runs along the bar edge on the flipaxis side. `out` points away from the bar
    // (figure units, y down); `pos(v)` is the position along the bar of value `v`.
    let (edge, out) = match (v, r.flipaxis) {
        (true, true) => (bar.right(), 1.0),
        (true, false) => (bar.x, -1.0),
        (false, true) => (bar.y, -1.0),
        (false, false) => (bar.bottom(), 1.0),
    };
    let span = f.hi - f.lo;
    let pos = |val: f64| {
        let t = (val - f.lo) / span;
        if v { bar.bottom() - t * bar.h } else { bar.x + t * bar.w }
    };
    // A tick of `size` at value `val`: from `0.5 sw - align * size` outside the edge, outward.
    let tick = |val: f64, size: f64, width: f64, align: f64, color: Color| {
        let a = edge + out * (0.5 * sw - align * size);
        let b = a + out * size;
        let p = pos(val);
        let rect = if v {
            Rect::new(a.min(b), p - 0.5 * width, size, width)
        } else {
            Rect::new(p - 0.5 * width, a.min(b), width, size)
        };
        RectPrim { rect, color, snap: true }
    };
    let mut ticks = Vec::new();
    if r.ticksvisible {
        for &t in &f.ticks.values {
            ticks.push(tick(t, r.ticksize, r.tickwidth, r.tickalign, r.tickcolor));
        }
    }
    if r.minorticksvisible {
        let minor = crate::ticks::resolve_minor(&r.minorticks, &f.ticks.values, f.lo, f.hi, Scale::Identity);
        for t in minor {
            ticks.push(tick(t, r.minorticksize, r.minortickwidth, r.minortickalign, r.minortickcolor));
        }
    }
    if !ticks.is_empty() {
        em.push(z::TICKS, None, Space::Figure, Prim::Rects(ticks));
    }

    // Tick labels: `spinewidth + tickspace + ticklabelpad` from the edge (Makie's LineAxis).
    let mut glyphs = Vec::new();
    if r.ticklabelsvisible {
        let gap = sw + f.tickspace() + r.ticklabelpad;
        let across = edge + out * gap;
        let align = match (v, r.flipaxis) {
            (true, true) => (0.0, 0.5),
            (true, false) => (1.0, 0.5),
            (false, true) => (0.5, 0.0),
            (false, false) => (0.5, 1.0),
        };
        for (val, label) in f.ticks.values.iter().zip(&f.ticks.labels) {
            let l = crate::text::layout(label, r.ticklabelsize, r.ticklabelfont, r.ticklabelcolor);
            let anchor = if v { [across, pos(*val)] } else { [pos(*val), across] };
            glyphs.extend(crate::text::place(&l, anchor, align, 0.0));
        }
    }

    // Label: `labelgap` from the edge, centered along the bar.
    if f.label_visible() {
        let labelgap = sw
            + f.tickspace()
            + if r.ticklabelsvisible { f.ticklabelspace() + r.ticklabelpad } else { 0.0 }
            + r.labelpadding;
        let rot = f.label_rotation();
        let l = crate::text::layout(&r.label, r.labelsize, r.labelfont, r.labelcolor);
        let middle = if v { bar.y + 0.5 * bar.h } else { bar.x + 0.5 * bar.w };
        let across = edge + out * labelgap;
        let mut anchor = if v { [across, middle] } else { [middle, across] };
        // Makie's automatic label alignment: the text's top or bottom faces the bar.
        let valign = match r.labelrotation {
            Some(_) => {
                // Explicit rotations center the label, shifted by half its extent away from the bar.
                let (w, h) = rotated_extent(l.width, l.height(), rot);
                if v {
                    anchor[0] += out * 0.5 * w;
                } else {
                    anchor[1] += out * 0.5 * h;
                }
                0.5
            }
            None if v => {
                if r.flipaxis != r.flip_vertical_label {
                    1.0
                } else {
                    0.0
                }
            }
            None => {
                if r.flipaxis {
                    0.0
                } else {
                    1.0
                }
            }
        };
        glyphs.extend(crate::text::place(&l, anchor, (0.5, valign), rot));
    }
    if !glyphs.is_empty() {
        em.push(z::TEXT, None, Space::Figure, Prim::Glyphs(GlyphsPrim { glyphs }));
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::scene::SceneCache;
    use crate::scene::drawlist::{DrawList, Prim, Rect};

    fn build(fig: &Figure) -> (DrawList, Vec<crate::scene::AxisFrame>) {
        crate::scene::build(&fig.sh.snapshot(), None, &mut SceneCache::new())
    }

    /// The gradient field of the first colorbar in the draw list: (x edges, y edges, nx, ny).
    fn field(dl: &DrawList) -> Option<([f64; 2], [f64; 2], u32, u32)> {
        dl.items.iter().find_map(|it| match &it.prim {
            Prim::Field(f) if it.space == crate::scene::drawlist::Space::Figure => {
                let (
                    crate::scene::drawlist::GridAxis::Regular { e0: x0, e1: x1 },
                    crate::scene::drawlist::GridAxis::Regular { e0: y0, e1: y1 },
                ) = (&f.x, &f.y)
                else {
                    return None;
                };
                Some(([*x0, *x1], [*y0, *y1], f.nx, f.ny))
            }
            _ => None,
        })
    }

    #[test]
    fn follows_heatmap_colorrange_and_clips() {
        let z: Vec<f64> = (0..12).map(|k| k as f64).collect();
        let fig = Figure::new();
        let hm = Axis::new(fig.at(1, 1)).heatmap(Field::new(&z, 4, 3)).colormap(Colormap::MAGMA);
        let cb = Colorbar::new(fig.at(1, 2), &hm);
        let m = cb.colormapping().unwrap();
        assert_eq!(m.colorrange, (0.0, 11.0));
        assert_eq!(m.colormap, Colormap::MAGMA);
        assert!(m.mapped);
        hm.colorrange((-1, 5)).highclip("red");
        let m = cb.colormapping().unwrap();
        assert_eq!((m.colorrange, m.highclip, m.lowclip), ((-1.0, 5.0), Some(Color::parse("red").unwrap()), None));
        // Colormap overrides are ignored for a linked colorbar.
        cb.colormap(Colormap::VIRIDIS);
        assert_eq!(cb.colormapping().unwrap().colormap, Colormap::MAGMA);
    }

    #[test]
    fn scatter_values_and_unmapped() {
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        let s = ax.scatter([1.0, 2.0, 3.0], [1.0, 2.0, 3.0]).color(vec![2.0, 4.0, 8.0]);
        let cb = Colorbar::new(fig.at(1, 2), &s);
        assert_eq!(cb.colormapping().unwrap().colorrange, (2.0, 8.0));
        let solid = ax.scatter([1.0], [1.0]).color("red");
        let cb2 = Colorbar::new(fig.at(1, 3), &solid);
        let m = cb2.colormapping().unwrap();
        assert!(!m.mapped);
        assert_eq!(m.colorrange, (0.0, 1.0));
        let l = ax.lines([1.0, 2.0], [1.0, 2.0]).color(vec![-1.0, 3.0]);
        assert_eq!(Colorbar::new(fig.at(2, 1), &l).colormapping().unwrap().colorrange, (-1.0, 3.0));
    }

    /// A vertical colorbar next to an axis: 12 units wide, as tall as the axis, flush with its
    /// spines, and the ticks/labels on the right.
    #[test]
    fn vertical_layout_lines_up_with_axis() {
        let z: Vec<f64> = (0..100).map(|k| k as f64 / 99.0).collect();
        let fig = Figure!(size = (600, 400));
        let hm = Axis::new(fig.at(1, 1)).heatmap(Field::new(&z, 10, 10));
        Colorbar::new(fig.at(1, 2), &hm).label("value");
        let (dl, axes) = build(&fig);
        let (x, y, nx, ny) = field(&dl).expect("colorbar gradient");
        assert_eq!((nx, ny), (1, 99));
        assert_eq!(x[1] - x[0], 12.0);
        let r: Rect = axes[0].rect;
        assert_eq!([y[1], y[0]], [r.y, r.bottom()], "bar spans the axis height");
        assert_eq!(x[0] - r.right(), 18.0, "colgap between the axis and the bar");
    }

    #[test]
    fn horizontal_and_clip_triangles_shorten_the_bar() {
        let fig = Figure!(size = (400, 300));
        Axis::new(fig.at(1, 1));
        Colorbar::from_colormap(fig.at(2, 1), Colormap::VIRIDIS, (0, 10))
            .vertical(false)
            .flipaxis(false)
            .lowclip("cyan")
            .highclip("red");
        let (dl, axes) = build(&fig);
        let (x, y, nx, ny) = field(&dl).unwrap();
        assert_eq!((nx, ny), (99, 1));
        assert_eq!(y[0] - y[1], 12.0);
        let tri = 12.0; // Makie shortens the bar by the full thickness
        let r = axes[0].rect;
        assert!((x[0] - (r.x + tri)).abs() < 1e-9 && (x[1] - (r.right() - tri)).abs() < 1e-9, "{x:?} {r:?}");
        assert!(dl.items.iter().any(|it| matches!(it.prim, Prim::Mesh(_))), "clip triangles");
        assert!(dl.items.iter().any(|it| matches!(it.prim, Prim::Lines(_))), "frame around the triangles");
    }

    #[test]
    fn protrusion_matches_makie() {
        // Ticks 0, 0.5, 1 ("0.0", "0.5", "1.0"), fontsize 14: protrusion = tickspace 5 + widest
        // label + pad 3 + (label height + labelpadding 5).
        let fig = Figure::new();
        let cb = Colorbar::from_colormap(fig.at(1, 1), Colormap::VIRIDIS, (0, 1)).label("x");
        let st = fig.sh.snapshot();
        let g = st.theme.globals();
        let ctx = crate::blocks::BlockCtx { st: &st, g: &g, axes: &[], id: cb.id };
        let Some(crate::blocks::Block::Colorbar(b)) = st.block(cb.id) else { panic!() };
        let f = b.frame(&ctx);
        let labels: Vec<String> = f.ticks.labels.iter().map(|l| l.plain_text()).collect();
        assert_eq!(labels, ["0.0", "0.5", "1.0"]);
        let w = crate::text::measure(&"0.0".into(), 14.0, Font::Regular).width;
        let expect = 5.0 + w + 3.0 + crate::text::line_height(14.0) + 5.0;
        let p = crate::blocks::BlockImpl::layout(&**b, &ctx).protrusion;
        assert!((p.right - expect).abs() < 1e-9 && p.left == 0.0, "{p:?} vs {expect}");
    }

    /// Protrusions and bar geometry printed by tools/colorbar_check.jl (CairoMakie) for the
    /// clips figure of examples/colorbar_check.rs.
    #[test]
    fn clips_figure_matches_cairomakie() {
        let fig = Figure!(size = (700, 400));
        let a = Axis::new(fig.at(1, 2));
        let w: Vec<f64> = (0..1200).map(|k| ((k % 40) as f64 * 0.3).sin() * 1.4).collect();
        let hm = a.heatmap(Field::new(&w, 40, 30)).colorrange((-1, 1)).lowclip("cyan").highclip("red");
        let right = Colorbar::new(fig.at(1, 3), &hm).label("clipped");
        let left = Colorbar::new(fig.at(1, 1), &hm).flipaxis(false).label("left side");
        let top = Colorbar::from_colormap(fig.at(Prepend, 2), Colormap::PLASMA, (0, 1000))
            .vertical(false)
            .highclip("black")
            .label("top");
        let st = fig.sh.snapshot();
        let g = st.theme.globals();
        let prot = |cb: &Colorbar| {
            let ctx = crate::blocks::BlockCtx { st: &st, g: &g, axes: &[], id: cb.id };
            let Some(crate::blocks::Block::Colorbar(b)) = st.block(cb.id) else { panic!() };
            let f = b.frame(&ctx);
            (f.protrusion(), f.ticks.values)
        };
        let (p, t) = prot(&right);
        assert!((p - 56.946).abs() < 1e-3, "{p}");
        assert_eq!(t, [-1.0, -0.5, 0.0, 0.5, 1.0]);
        assert!((prot(&left).0 - 56.946).abs() < 1e-3);
        let (p, t) = prot(&top);
        assert!((p - 45.62).abs() < 1e-3, "{p}");
        assert_eq!(t, [0.0, 500.0, 1000.0]);
        // Makie: top colorbar bbox (l, b, w, h) = (127.51, 326.38, 469.54, 12): rounded to
        // x 128..597, y 62..74 (y down); the high clip shortens the bar by 12.
        let (dl, _) = build(&fig);
        let fields: Vec<_> = dl
            .items
            .iter()
            .filter(|it| it.space == crate::scene::drawlist::Space::Figure)
            .filter_map(|it| match &it.prim {
                Prim::Field(f) => Some(f),
                _ => None,
            })
            .collect();
        let horizontal = fields.iter().find(|f| f.ny == 1).unwrap();
        let crate::scene::drawlist::GridAxis::Regular { e0, e1 } = horizontal.x else { panic!() };
        assert_eq!((e0, e1), (128.0, 585.0));
    }

    /// The colorbar reads the plot at every frame: new data moves its automatic colorrange.
    #[test]
    fn live_updates_propagate() {
        let fig = Figure::new();
        let hm = Axis::new(fig.at(1, 1)).heatmap(Field::new(&[0.0, 1.0, 2.0, 3.0], 2, 2));
        let cb = Colorbar::new(fig.at(1, 2), &hm);
        let ticks = || {
            let st = fig.sh.snapshot();
            let g = st.theme.globals();
            let ctx = crate::blocks::BlockCtx { st: &st, g: &g, axes: &[], id: cb.id };
            let Some(crate::blocks::Block::Colorbar(b)) = st.block(cb.id) else { panic!() };
            b.frame(&ctx).ticks.values
        };
        assert_eq!(ticks(), [0.0, 1.0, 2.0, 3.0]);
        hm.set_data(Field::new(&[0.0, 100.0, 50.0, 20.0], 2, 2));
        assert_eq!(ticks(), [0.0, 50.0, 100.0]);
        assert_eq!(cb.colormapping().unwrap().colorrange, (0.0, 100.0));
        hm.delete();
        assert_eq!(cb.colormapping().unwrap().colorrange, (0.0, 1.0));
        let _ = build(&fig);
    }
}
