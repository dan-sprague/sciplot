//! Axis limits, protrusions and decorations (background, grid, ticks, spines, labels, title).

use super::AxisFrame;
use super::drawlist::{AxisXform, Emitter, GlyphsPrim, Prim, Rect, RectPrim, Space};
use crate::blocks::Block;
use crate::blocks::axis::{Aspect, AxisResolved};
use crate::figure::{BlockId, FigState};
use crate::layout::Protrusion;
use crate::text::{RichText, TextLayout};
use crate::theme::Globals;
use crate::ticks::Ticks;
use crate::transform::Scale;

/// Draw-order constants (Makie's axis scene conventions).
pub(crate) mod z {
    pub const BACKGROUND: f32 = -100.0;
    pub const GRID: f32 = -10.0;
    pub const TICKS: f32 = 10.0;
    pub const SPINES: f32 = 20.0;
    pub const TEXT: f32 = 30.0;
}

/// Finite bounds `(lo, hi)` in scaled space.
type Bounds = Option<(f64, f64)>;

fn union(a: Bounds, b: Bounds) -> Bounds {
    match (a, b) {
        (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.max(b.1))),
        (x, None) | (None, x) => x,
    }
}

/// Makie's `expandlimits`: margins in scaled space, degenerate ranges widened. Returns data space.
pub(crate) fn expand(b: (f64, f64), margin: [f64; 2], scale: Scale) -> (f64, f64) {
    let (sa, sb) = b;
    let w = sb - sa;
    let (lo, hi) = (sa - w * margin[0], sb + w * margin[1]);
    if hi - lo == 0.0 {
        let zd = lo.abs();
        if zd == 0.0 {
            return match scale {
                Scale::Identity | Scale::Sqrt => (
                    scale.inverse(-1.0).min(0.0).max(if scale == Scale::Sqrt { 0.0 } else { -1.0 }),
                    scale.inverse(1.0),
                ),
                // log of 1: widen a decade each way (Makie leaves this singular).
                _ => (scale.inverse(lo) / 10.0, scale.inverse(lo) * 10.0),
            };
        }
        return (scale.inverse(lo - zd), scale.inverse(lo + zd));
    }
    (scale.inverse(lo), scale.inverse(hi))
}

fn default_limits(scale: Scale) -> (f64, f64) {
    match scale {
        Scale::Identity => (0.0, 10.0),
        Scale::Sqrt => (0.0, 100.0),
        _ => (1.0, 1000.0),
    }
}

/// Visible limits (data space, ordered) for each axis in `ids`, in order.
pub(crate) fn compute_limits(st: &FigState, ids: &[BlockId], g: &Globals) -> Vec<[f64; 4]> {
    // Per-axis resolved attributes and raw data bounds in scaled space.
    let resolved: Vec<AxisResolved> = ids
        .iter()
        .map(|id| st.block(*id).and_then(|b| b.as_axis()).unwrap().attrs.resolve(&st.theme.axis, g))
        .collect();
    let raw: Vec<(Bounds, Bounds, bool)> = ids
        .iter()
        .zip(&resolved)
        .map(|(id, r)| {
            let ax = st.block(*id).and_then(|b| b.as_axis()).unwrap();
            let mut bx: Bounds = None;
            let mut by: Bounds = None;
            let mut tight = false;
            for pid in &ax.plots {
                if let Some(p) = st.plot(*pid) {
                    if !p.common.visible {
                        continue;
                    }
                    tight |= p.kind.imp().tight_limits();
                    if let Some([x0, x1, y0, y1]) = p.kind.imp().data_bounds(r.xscale, r.yscale) {
                        if p.common.xautolimits && x0.is_finite() {
                            bx = union(bx, Some((x0, x1)));
                        }
                        if p.common.yautolimits && y0.is_finite() {
                            by = union(by, Some((y0, y1)));
                        }
                    }
                }
            }
            (bx, by, tight)
        })
        .collect();

    let index_of = |id: &BlockId| ids.iter().position(|i| i == id);
    ids.iter()
        .enumerate()
        .map(|(i, id)| {
            let ax = st.block(*id).and_then(|b| b.as_axis()).unwrap();
            let r = &resolved[i];
            if let (Some(v), false) = (ax.interactive, ax.follow) {
                return v;
            }
            // Union with linked axes before margins. Linked axes share one set of limits (Makie
            // propagates the last reset); the group is tight only if every member is (heatmaps),
            // otherwise it keeps the largest margin of its non-tight members.
            let own = |j: usize, x: bool| -> Option<[f64; 2]> {
                (!raw[j].2).then(|| if x { resolved[j].xautolimitmargin } else { resolved[j].yautolimitmargin })
            };
            let widest = |a: Option<[f64; 2]>, b: Option<[f64; 2]>| match (a, b) {
                (Some(a), Some(b)) => Some([a[0].max(b[0]), a[1].max(b[1])]),
                (x, None) | (None, x) => x,
            };
            let mut bx = raw[i].0;
            let mut by = raw[i].1;
            let mut mx = own(i, true);
            let mut my = own(i, false);
            for l in &ax.xlinks {
                if let Some(j) = index_of(l) {
                    bx = union(bx, raw[j].0);
                    mx = widest(mx, own(j, true));
                }
            }
            for l in &ax.ylinks {
                if let Some(j) = index_of(l) {
                    by = union(by, raw[j].1);
                    my = widest(my, own(j, false));
                }
            }
            let (mx, my) = (mx.unwrap_or([0.0; 2]), my.unwrap_or([0.0; 2]));
            let (mut x0, mut x1) = bx.map_or(default_limits(r.xscale), |b| expand(b, mx, r.xscale));
            let (mut y0, mut y1) = by.map_or(default_limits(r.yscale), |b| expand(b, my, r.yscale));
            // User limits (possibly partial) win; a linked axis' user x limits apply too.
            let mut xl = ax.xlims;
            for l in &ax.xlinks {
                if let Some(o) = st.block(*l).and_then(|b| b.as_axis()) {
                    xl.0 = xl.0.or(o.xlims.0);
                    xl.1 = xl.1.or(o.xlims.1);
                }
            }
            let mut yl = ax.ylims;
            for l in &ax.ylinks {
                if let Some(o) = st.block(*l).and_then(|b| b.as_axis()) {
                    yl.0 = yl.0.or(o.ylims.0);
                    yl.1 = yl.1.or(o.ylims.1);
                }
            }
            if let Some(v) = xl.0 {
                x0 = v;
            }
            if let Some(v) = xl.1 {
                x1 = v;
            }
            if let Some(v) = yl.0 {
                y0 = v;
            }
            if let Some(v) = yl.1 {
                y1 = v;
            }
            if x0 > x1 {
                std::mem::swap(&mut x0, &mut x1);
            }
            if y0 > y1 {
                std::mem::swap(&mut y0, &mut y1);
            }
            if x0 == x1 {
                (x0, x1) = expand((r.xscale.forward(x0), r.xscale.forward(x1)), [0.0; 2], r.xscale);
            }
            if y0 == y1 {
                (y0, y1) = expand((r.yscale.forward(y0), r.yscale.forward(y1)), [0.0; 2], r.yscale);
            }
            [x0, x1, y0, y1]
        })
        .collect()
}
/// Makie's `adjustlimits!` for `autolimitaspect`: widens the x or the y limits (data space, split
/// like the autolimit margins) so that one data unit has `aspect` times the length on x as on y
/// for an axis area of `w × h` units. Returns the limits unchanged when there is nothing to do.
pub(crate) fn adjust_limits_for_aspect(lims: [f64; 4], a: &AxisResolved, w: f64, h: f64) -> [f64; 4] {
    let Some(asp) = a.autolimitaspect else { return lims };
    let [x0, x1, y0, y1] = lims;
    if !(w > 0.0 && h > 0.0 && asp > 0.0 && asp.is_finite()) || x1 <= x0 || y1 <= y0 {
        return lims;
    }
    let correction = asp / ((x1 - x0) / (y1 - y0) / (w / h));
    let ratios = |m: [f64; 2]| {
        let s = m[0] + m[1];
        if s == 0.0 { [0.5, 0.5] } else { [m[0] / s, m[1] / s] }
    };
    let widen = |lo: f64, hi: f64, f: f64, m: [f64; 2]| {
        let r = ratios(m);
        let d = hi - lo;
        (lo - d * (f - 1.0) * r[0], hi + d * (f - 1.0) * r[1])
    };
    let out = if correction > 1.0 {
        let (a0, a1) = widen(x0, x1, correction, a.xautolimitmargin);
        [a0, a1, y0, y1]
    } else if correction < 1.0 {
        let (b0, b1) = widen(y0, y1, 1.0 / correction, a.yautolimitmargin);
        [x0, x1, b0, b1]
    } else {
        lims
    };
    let ok = a.xscale.forward(out[0]).is_finite() && a.yscale.forward(out[2]).is_finite();
    if ok && out.iter().all(|v| v.is_finite()) { out } else { lims }
}

/// Makie's `sceneareanode!`: the axis area inside its cell `r` for `aspect` (centred, then rounded
/// to integer units like Makie's `round_to_IRect2D`; `fig_h` is the figure height, since Makie
/// rounds in its y-up coordinates).
pub(crate) fn aspect_area(r: Rect, aspect: Option<Aspect>, limits: [f64; 4], fig_h: f64) -> Rect {
    let ratio = match aspect {
        None => None,
        Some(Aspect::Axis(v)) => Some(v),
        Some(Aspect::Data) => Some((limits[1] - limits[0]) / (limits[3] - limits[2])),
    };
    let (mut w, mut h) = (r.w, r.h);
    if let Some(ratio) = ratio.filter(|v| v.is_finite() && *v > 0.0) {
        let as_ = r.w / r.h;
        if as_ >= ratio {
            w *= ratio / as_;
        } else {
            h *= as_ / ratio;
        }
    }
    let l = r.x + 0.5 * (r.w - w);
    let b = (fig_h - r.bottom()) + 0.5 * (r.h - h);
    let bb = crate::layout::BBox { l, r: l + w, b, t: b + h };
    if !(bb.l.is_finite() && bb.r.is_finite() && bb.b.is_finite() && bb.t.is_finite()) {
        return r;
    }
    bb.round().to_rect(fig_h)
}

fn is_blank(t: &RichText) -> bool {
    t.spans.iter().all(|s| s.text.chars().all(char::is_whitespace))
}

fn inside(v: f64, lo: f64, hi: f64) -> bool {
    let eps = 1e-9 * (hi - lo).abs();
    v >= lo.min(hi) - eps && v <= lo.max(hi) + eps
}

/// Makie's `calculate_real_ticklabel_align` for automatic alignment, as `(h, v)` fractions
/// (h: 0 left .. 1 right, v: 0 bottom .. 1 top) for an x axis at the bottom / y axis at the left.
fn ticklabel_align(horizontal: bool, rot: f64) -> (f64, f64) {
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-6 * b.abs().max(1.0);
    if rot == 0.0 || !rot.is_finite() {
        if horizontal { (0.5, 1.0) } else { (1.0, 0.5) }
    } else if near(rot, std::f64::consts::FRAC_PI_2) {
        if horizontal { (1.0, 0.5) } else { (0.5, 0.0) }
    } else if near(rot, -std::f64::consts::FRAC_PI_2) {
        if horizontal { (0.0, 0.5) } else { (0.5, 1.0) }
    } else if rot > 0.0 {
        if horizontal { (1.0, 1.0) } else { (1.0, 0.5) }
    } else if horizontal {
        (0.0, 1.0)
    } else {
        (1.0, 0.5)
    }
}

/// One axis side's tick labels that fall inside the limits, laid out.
fn tick_label_layouts(
    t: &Ticks,
    lo: f64,
    hi: f64,
    size: f64,
    font: crate::text::Font,
    color: crate::color::Color,
) -> Vec<(f64, TextLayout)> {
    t.values
        .iter()
        .zip(&t.labels)
        .filter(|(v, _)| inside(**v, lo, hi))
        .map(|(v, l)| (*v, crate::text::layout(l, size, font, color)))
        .collect()
}

/// Makie's automatic `ticklabelspace`: the extent normal to the axis of the union of the visible
/// tick labels' boxes (0 without labels).
fn ideal_ticklabel_space(labels: &[(f64, TextLayout)], horizontal: bool, rot: f64) -> f64 {
    let align = ticklabel_align(horizontal, rot);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for (_, l) in labels {
        let b = crate::text::placed_bbox(l, [0.0, 0.0], align, rot);
        let (a, z) = if horizontal { (b[1], b[1] + b[3]) } else { (b[0], b[0] + b[2]) };
        lo = lo.min(a);
        hi = hi.max(z);
    }
    if hi >= lo { hi - lo } else { 0.0 }
}

/// Per-side quantities of Makie's `LineAxis` (x at the bottom, y at the left).
struct Side {
    /// Tick labels inside the limits.
    labels: Vec<(f64, TextLayout)>,
    /// `max(0, ticksize·(1 − tickalign))` when ticks are visible.
    tickspace: f64,
    /// `actual_ticklabelspace`.
    labelspace: f64,
}

fn side(a: &AxisResolved, t: &Ticks, lo: f64, hi: f64, x: bool) -> Side {
    let (size, font, color, rot, visible, space, ticksvisible, ticksize, tickalign) = if x {
        (
            a.xticklabelsize,
            a.xticklabelfont,
            a.xticklabelcolor,
            a.xticklabelrotation,
            a.xticklabelsvisible,
            a.xticklabelspace,
            a.xticksvisible,
            a.xticksize,
            a.xtickalign,
        )
    } else {
        (
            a.yticklabelsize,
            a.yticklabelfont,
            a.yticklabelcolor,
            a.yticklabelrotation,
            a.yticklabelsvisible,
            a.yticklabelspace,
            a.yticksvisible,
            a.yticksize,
            a.ytickalign,
        )
    };
    let labels = tick_label_layouts(t, lo, hi, size, font, color);
    let ideal = if visible { ideal_ticklabel_space(&labels, x, rot) } else { 0.0 };
    Side {
        tickspace: if ticksvisible { (ticksize * (1.0 - tickalign)).max(0.0) } else { 0.0 },
        labelspace: space.unwrap_or(ideal),
        labels,
    }
}

/// Makie's `actual_ticklabelspace` of a built axis, `[x, y]` in units (what the window freezes
/// while the user zooms, so the layout does not jitter).
pub(crate) fn actual_ticklabelspace(a: &AxisFrame) -> [f64; 2] {
    let x = side(&a.attrs, &a.xticks, a.limits[0], a.limits[1], true).labelspace;
    let y = side(&a.attrs, &a.yticks, a.limits[2], a.limits[3], false).labelspace;
    [x, y]
}

/// Height (x label) or width (rotated y label) of an axis label's box.
fn label_extent(t: &RichText, size: f64, font: crate::text::Font) -> f64 {
    crate::text::layout(t, size, font, crate::color::Color::TRANSPARENT).height()
}

/// Makie's `LineAxis` protrusion: `tickspace + (ticklabelspace + ticklabelpad) + (label + labelpadding)`
/// (spinewidth is not included), plus the title for the top.
pub(crate) fn protrusion(a: &AxisResolved, xt: &Ticks, yt: &Ticks, limits: [f64; 4]) -> Protrusion {
    let xs = side(a, xt, limits[0], limits[1], true);
    let ys = side(a, yt, limits[2], limits[3], false);
    let lineaxis = |s: &Side,
                    ticksvisible: bool,
                    labelsvisible: bool,
                    pad: f64,
                    label: &RichText,
                    lv: bool,
                    lsize,
                    lfont,
                    lpad| {
        let tick = if ticksvisible && !s.labels.is_empty() { s.tickspace } else { 0.0 };
        let gap = if labelsvisible && s.labelspace > 0.0 { s.labelspace + pad } else { 0.0 };
        let lab = if lv && !is_blank(label) { label_extent(label, lsize, lfont) + lpad } else { 0.0 };
        tick + gap + lab
    };
    let bottom = lineaxis(
        &xs,
        a.xticksvisible,
        a.xticklabelsvisible,
        a.xticklabelpad,
        &a.xlabel,
        a.xlabelvisible,
        a.xlabelsize,
        a.xlabelfont,
        a.xlabelpadding,
    );
    let left = lineaxis(
        &ys,
        a.yticksvisible,
        a.yticklabelsvisible,
        a.yticklabelpad,
        &a.ylabel,
        a.ylabelvisible,
        a.ylabelsize,
        a.ylabelfont,
        a.ylabelpadding,
    );
    let mut top = 0.0;
    if a.titlevisible && !is_blank(&a.title) {
        top += label_extent(&a.title, a.titlesize, a.titlefont) + a.titlegap;
    }
    if a.subtitlevisible && !is_blank(&a.subtitle) {
        top += label_extent(&a.subtitle, a.subtitlesize, a.subtitlefont) + a.subtitlegap;
    }
    Protrusion { left, right: 0.0, bottom, top }
}

/// Unit x of a data x value.
fn ux(a: &AxisFrame, x: &AxisXform, v: f64) -> f64 {
    let s = a.attrs.xscale.forward(v);
    let l = (s - a.rebase.origin[0]) * a.rebase.k[0];
    x.rect.x + (l - x.view[0]) / (x.view[1] - x.view[0]) * x.rect.w
}

/// Unit y of a data y value.
fn uy(a: &AxisFrame, x: &AxisXform, v: f64) -> f64 {
    let s = a.attrs.yscale.forward(v);
    let l = (s - a.rebase.origin[1]) * a.rebase.k[1];
    x.rect.bottom() - (l - x.view[2]) / (x.view[3] - x.view[2]) * x.rect.h
}

/// What an axis text element is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextKind {
    XTickLabel,
    YTickLabel,
    XLabel,
    YLabel,
    Title,
    Subtitle,
}

/// One placed text element of an axis' decorations.
pub(crate) struct AxisText {
    pub kind: TextKind,
    pub layout: TextLayout,
    pub anchor: [f64; 2],
    pub align: (f64, f64),
    pub angle: f64,
}

impl AxisText {
    /// Its box `[x, y, w, h]` in figure units (y down).
    pub fn bbox(&self) -> [f64; 4] {
        crate::text::placed_bbox(&self.layout, self.anchor, self.align, self.angle)
    }
}

/// The axis' texts at Makie's positions (`LineAxis` and `calculate_title_position`):
/// - tick labels at `spine + spinewidth + tickspace + ticklabelpad`, aligned (center, top) on x
///   and (right, center) on y;
/// - axis labels at `spine + spinewidth + tickspace + (ticklabelspace + ticklabelpad) +
///   labelpadding`, centred along the axis; the y label is rotated +90° with its bottom facing
///   the axis;
/// - the subtitle at `top + titlegap`, the title above it, both bottom-aligned.
pub(crate) fn decoration_texts(a: &AxisFrame, xf: &AxisXform) -> Vec<AxisText> {
    let r = a.rect;
    let at = &a.attrs;
    let sw = at.spinewidth;
    let mut out = Vec::new();

    let xs = side(at, &a.xticks, a.limits[0], a.limits[1], true);
    if at.xticklabelsvisible {
        let y = r.bottom() + sw + xs.tickspace + at.xticklabelpad;
        let align = ticklabel_align(true, at.xticklabelrotation);
        for (v, layout) in xs.labels.iter().cloned() {
            out.push(AxisText {
                kind: TextKind::XTickLabel,
                layout,
                anchor: [ux(a, xf, v), y],
                align,
                angle: at.xticklabelrotation,
            });
        }
    }
    if at.xlabelvisible && !is_blank(&at.xlabel) {
        let gap = sw
            + xs.tickspace
            + if at.xticklabelsvisible { xs.labelspace + at.xticklabelpad } else { 0.0 }
            + at.xlabelpadding;
        out.push(AxisText {
            kind: TextKind::XLabel,
            layout: crate::text::layout(&at.xlabel, at.xlabelsize, at.xlabelfont, at.xlabelcolor),
            anchor: [r.x + 0.5 * r.w, r.bottom() + gap],
            align: (0.5, 1.0),
            angle: 0.0,
        });
    }

    let ys = side(at, &a.yticks, a.limits[2], a.limits[3], false);
    if at.yticklabelsvisible {
        let x = r.x - sw - ys.tickspace - at.yticklabelpad;
        let align = ticklabel_align(false, at.yticklabelrotation);
        for (v, layout) in ys.labels.iter().cloned() {
            out.push(AxisText {
                kind: TextKind::YTickLabel,
                layout,
                anchor: [x, uy(a, xf, v)],
                align,
                angle: at.yticklabelrotation,
            });
        }
    }
    if at.ylabelvisible && !is_blank(&at.ylabel) {
        let gap = sw
            + ys.tickspace
            + if at.yticklabelsvisible { ys.labelspace + at.yticklabelpad } else { 0.0 }
            + at.ylabelpadding;
        out.push(AxisText {
            kind: TextKind::YLabel,
            layout: crate::text::layout(&at.ylabel, at.ylabelsize, at.ylabelfont, at.ylabelcolor),
            anchor: [r.x - gap, r.y + 0.5 * r.h],
            align: (0.5, 0.0),
            angle: std::f64::consts::FRAC_PI_2,
        });
    }

    let f = at.titlealign.frac();
    let mut y = r.y - at.titlegap;
    if at.subtitlevisible && !is_blank(&at.subtitle) {
        let layout = crate::text::layout(&at.subtitle, at.subtitlesize, at.subtitlefont, at.subtitlecolor);
        y -= layout.height() + at.subtitlegap;
        out.push(AxisText {
            kind: TextKind::Subtitle,
            layout,
            anchor: [r.x + f * r.w, r.y - at.titlegap],
            align: (f, 0.0),
            angle: 0.0,
        });
    }
    if at.titlevisible && !is_blank(&at.title) {
        out.push(AxisText {
            kind: TextKind::Title,
            layout: crate::text::layout(&at.title, at.titlesize, at.titlefont, at.titlecolor),
            anchor: [r.x + f * r.w, y],
            align: (f, 0.0),
            angle: 0.0,
        });
    }
    out
}

pub(crate) fn emit_decorations(em: &mut Emitter, a: &AxisFrame, xf: &AxisXform) {
    let r = a.rect;
    let at = &a.attrs;
    let rects = |v: Vec<RectPrim>| Prim::Rects(v);

    em.push(
        z::BACKGROUND,
        None,
        Space::Figure,
        rects(vec![RectPrim { rect: r, color: at.backgroundcolor, snap: false }]),
    );

    let xs: Vec<f64> = a.xticks.values.iter().copied().filter(|v| inside(*v, a.limits[0], a.limits[1])).collect();
    let ys: Vec<f64> = a.yticks.values.iter().copied().filter(|v| inside(*v, a.limits[2], a.limits[3])).collect();
    let xminor = if at.xminorticksvisible || at.xminorgridvisible {
        crate::ticks::resolve_minor(&at.xminorticks, &a.xticks.values, a.limits[0], a.limits[1], at.xscale)
    } else {
        vec![]
    };
    let yminor = if at.yminorticksvisible || at.yminorgridvisible {
        crate::ticks::resolve_minor(&at.yminorticks, &a.yticks.values, a.limits[2], a.limits[3], at.yscale)
    } else {
        vec![]
    };

    // Grid (clipped to the axis).
    let mut grid = Vec::new();
    let vline = |v: f64, width: f64, color, out: &mut Vec<RectPrim>| {
        let x = ux(a, xf, v);
        out.push(RectPrim { rect: Rect::new(x - 0.5 * width, r.y, width, r.h), color, snap: true });
    };
    let hline = |v: f64, width: f64, color, out: &mut Vec<RectPrim>| {
        let y = uy(a, xf, v);
        out.push(RectPrim { rect: Rect::new(r.x, y - 0.5 * width, r.w, width), color, snap: true });
    };
    if at.xminorgridvisible {
        for &v in &xminor {
            vline(v, at.xminorgridwidth, at.xminorgridcolor, &mut grid);
        }
    }
    if at.yminorgridvisible {
        for &v in &yminor {
            hline(v, at.yminorgridwidth, at.yminorgridcolor, &mut grid);
        }
    }
    if at.xgridvisible {
        for &v in &xs {
            vline(v, at.xgridwidth, at.xgridcolor, &mut grid);
        }
    }
    if at.ygridvisible {
        for &v in &ys {
            hline(v, at.ygridwidth, at.ygridcolor, &mut grid);
        }
    }
    if !grid.is_empty() {
        em.push(z::GRID, Some(r), Space::Figure, rects(grid));
    }

    // Ticks (Makie's `update_tick_obs`): they start half a spine width outside the axis edge,
    // `tickalign` moves them inward; mirrored ticks sit on the opposite spine.
    let sw = at.spinewidth;
    let mut ticks = Vec::new();
    let xtick = |v: f64, size: f64, align: f64, width: f64, color, out: &mut Vec<RectPrim>| {
        let x = ux(a, xf, v);
        let y0 = r.bottom() + 0.5 * sw - size * align;
        out.push(RectPrim { rect: Rect::new(x - 0.5 * width, y0, width, size), color, snap: true });
        if at.xticksmirrored {
            let y0 = r.y - 0.5 * sw - size * (1.0 - align);
            out.push(RectPrim { rect: Rect::new(x - 0.5 * width, y0, width, size), color, snap: true });
        }
    };
    if at.xticksvisible {
        for &v in &xs {
            xtick(v, at.xticksize, at.xtickalign, at.xtickwidth, at.xtickcolor, &mut ticks);
        }
    }
    if at.xminorticksvisible {
        for &v in &xminor {
            xtick(v, at.xminorticksize, at.xminortickalign, at.xminortickwidth, at.xminortickcolor, &mut ticks);
        }
    }
    let ytick = |v: f64, size: f64, align: f64, width: f64, color, out: &mut Vec<RectPrim>| {
        let y = uy(a, xf, v);
        let x0 = r.x - 0.5 * sw - size * (1.0 - align);
        out.push(RectPrim { rect: Rect::new(x0, y - 0.5 * width, size, width), color, snap: true });
        if at.yticksmirrored {
            let x0 = r.right() + 0.5 * sw - size * align;
            out.push(RectPrim { rect: Rect::new(x0, y - 0.5 * width, size, width), color, snap: true });
        }
    };
    if at.yticksvisible {
        for &v in &ys {
            ytick(v, at.yticksize, at.ytickalign, at.ytickwidth, at.ytickcolor, &mut ticks);
        }
    }
    if at.yminorticksvisible {
        for &v in &yminor {
            ytick(v, at.yminorticksize, at.yminortickalign, at.yminortickwidth, at.yminortickcolor, &mut ticks);
        }
    }
    if !ticks.is_empty() {
        em.push(z::TICKS, None, Space::Figure, rects(ticks));
    }

    // Spines, centered on the axis boundary.
    let mut sp = Vec::new();
    if at.leftspinevisible {
        sp.push(RectPrim {
            rect: Rect::new(r.x - 0.5 * sw, r.y - 0.5 * sw, sw, r.h + sw),
            color: at.leftspinecolor,
            snap: true,
        });
    }
    if at.rightspinevisible {
        sp.push(RectPrim {
            rect: Rect::new(r.right() - 0.5 * sw, r.y - 0.5 * sw, sw, r.h + sw),
            color: at.rightspinecolor,
            snap: true,
        });
    }
    if at.bottomspinevisible {
        sp.push(RectPrim {
            rect: Rect::new(r.x - 0.5 * sw, r.bottom() - 0.5 * sw, r.w + sw, sw),
            color: at.bottomspinecolor,
            snap: true,
        });
    }
    if at.topspinevisible {
        sp.push(RectPrim {
            rect: Rect::new(r.x - 0.5 * sw, r.y - 0.5 * sw, r.w + sw, sw),
            color: at.topspinecolor,
            snap: true,
        });
    }
    if !sp.is_empty() {
        em.push(z::SPINES, None, Space::Figure, rects(sp));
    }

    // Text: tick labels, axis labels, title.
    let glyphs: Vec<_> = decoration_texts(a, xf)
        .iter()
        .flat_map(|t| crate::text::place(&t.layout, t.anchor, t.align, t.angle))
        .collect();
    if !glyphs.is_empty() {
        em.push(z::TEXT, None, Space::Figure, Prim::Glyphs(GlyphsPrim { glyphs }));
    }
}

/// Geometry of an axis as laid out (figure units, y down), for fidelity checks against Makie.
#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AxisGeometry {
    /// The axis area inside the spines: `[x, y, w, h]`.
    pub viewport: [f64; 4],
    /// Final limits `[x0, x1, y0, y1]` (data space).
    pub limits: [f64; 4],
    /// Boxes `[x, y, w, h]` of the visible x tick labels, left to right.
    pub xticklabels: Vec<[f64; 4]>,
    /// Boxes of the visible y tick labels, bottom to top.
    pub yticklabels: Vec<[f64; 4]>,
    pub xlabel: Option<[f64; 4]>,
    pub ylabel: Option<[f64; 4]>,
    pub title: Option<[f64; 4]>,
    pub subtitle: Option<[f64; 4]>,
}

pub(crate) fn geometry(a: &AxisFrame, xf: &AxisXform) -> AxisGeometry {
    let r = a.rect;
    let mut g = AxisGeometry { viewport: [r.x, r.y, r.w, r.h], limits: a.limits, ..Default::default() };
    for t in decoration_texts(a, xf) {
        let b = t.bbox();
        match t.kind {
            TextKind::XTickLabel => g.xticklabels.push(b),
            TextKind::YTickLabel => g.yticklabels.push(b),
            TextKind::XLabel => g.xlabel = Some(b),
            TextKind::YLabel => g.ylabel = Some(b),
            TextKind::Title => g.title = Some(b),
            TextKind::Subtitle => g.subtitle = Some(b),
        }
    }
    g.xticklabels.sort_by(|p, q| p[0].total_cmp(&q[0]));
    g.yticklabels.sort_by(|p, q| q[1].total_cmp(&p[1]));
    g
}

#[allow(dead_code)]
pub(crate) fn is_axis(b: &Block) -> bool {
    matches!(b, Block::Axis(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;

    fn frames(fig: &Figure) -> Vec<AxisFrame> {
        crate::scene::build(&fig.sh.snapshot(), None, &mut crate::scene::SceneCache::new()).1
    }

    /// CairoMakie (tools/fidelity_check.jl): a 40×20 DataAspect heatmap in a default figure has
    /// the viewport (41, 91) 543×271 (y up), i.e. y = 450 - 91 - 271 = 88 down.
    #[test]
    fn data_aspect_shrinks_the_axis_centred() {
        let z = vec![0.0; 800];
        let hm = heatmap(Field::new(&z, 40, 20));
        hm.axis().aspect(DataAspect).title("DataAspect");
        assert_eq!(frames(&hm.figure())[0].rect, Rect::new(41.0, 88.0, 543.0, 271.0));
        // AxisAspect(1): square (to the rounding of its corners), centred horizontally in the
        // cell; a bare number means the same.
        hm.axis().aspect(AxisAspect(1.0));
        let r = frames(&hm.figure())[0].rect;
        assert!((r.w - r.h).abs() <= 1.0 && r.h >= 374.0, "{r:?}");
        assert!(((r.x + 0.5 * r.w) - (41.0 + 0.5 * 543.0)).abs() <= 1.0, "{r:?}");
        hm.axis().aspect(1);
        assert_eq!(frames(&hm.figure())[0].rect, r);
        hm.axis().aspect(None);
        assert_eq!(frames(&hm.figure())[0].rect.w, 543.0);
    }

    /// CairoMakie: a unit circle with autolimitaspect = 1 gets x limits ±1.5576 (the axis keeps
    /// its 531 × 375 cell and the x range widens).
    #[test]
    fn autolimitaspect_widens_limits() {
        let th: Vec<f64> = (0..100).map(|i| std::f64::consts::TAU * i as f64 / 99.0).collect();
        let l = lines(th.iter().map(|t| t.cos()), th.iter().map(|t| t.sin()));
        l.axis().autolimitaspect(1).title("autolimitaspect");
        let a = &frames(&l.figure())[0];
        assert_eq!((a.rect.w, a.rect.h), (531.0, 375.0));
        let want = [-1.5571522145661592, 1.557655672182974, -1.0998615419311912, 1.0998615419311912];
        for (g, w) in a.limits.iter().zip(want) {
            assert!((g - w).abs() < 1e-6, "{:?}", a.limits);
        }
        let per_unit = |i: usize, len: f64| len / (a.limits[i + 1] - a.limits[i]);
        assert!((per_unit(0, a.rect.w) - per_unit(2, a.rect.h)).abs() < 1e-9);
    }

    #[test]
    fn adjust_limits_splits_by_margins() {
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1)).autolimitaspect(2).xautolimitmargin((0.0, 0.1));
        let st = fig.sh.snapshot();
        let r = st.block(ax.id).and_then(|b| b.as_axis()).unwrap().attrs.resolve(&st.theme.axis, &st.theme.globals());
        // 100 × 100 area, 1 × 1 limits: x must span 2 units, all of the extra on the right.
        let l = adjust_limits_for_aspect([0.0, 1.0, 0.0, 1.0], &r, 100.0, 100.0);
        assert!((l[0] - 0.0).abs() < 1e-12 && (l[1] - 2.0).abs() < 1e-12 && l[2..] == [0.0, 1.0], "{l:?}");
        // Too wide: y widens symmetrically (default margins 0.05 / 0.05).
        let l = adjust_limits_for_aspect([0.0, 4.0, 0.0, 1.0], &r, 100.0, 100.0);
        assert!((l[2] + 0.5).abs() < 1e-12 && (l[3] - 1.5).abs() < 1e-12, "{l:?}");
    }

    #[test]
    fn resize_to_layout_fits_fixed_axes() {
        let fig = Figure::new();
        Axis::new(fig.at(1, 1)).width(300).height(200);
        let prot = {
            let a = &frames(&fig)[0];
            protrusion(&a.attrs, &a.xticks, &a.yticks, a.limits)
        };
        fig.resize_to_layout();
        let size = fig.sh.snapshot().theme.globals().size;
        let want = |a: f64| a.round_ties_even();
        assert_eq!(size, [want(300.0 + prot.left + 32.0), want(200.0 + prot.bottom + prot.top + 32.0)]);
        let r = frames(&fig)[0].rect;
        assert_eq!((r.w, r.h), (300.0, 200.0));
    }

    /// Makie's `LineAxis`: tick labels sit `spinewidth + ticksize + ticklabelpad` outside the
    /// spine, the x label below them.
    #[test]
    fn decoration_offsets_include_spinewidth() {
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1)).xlabel("x").spinewidth(3);
        ax.lines([0.0, 1.0], [0.0, 1.0]);
        let (dl, axes) = crate::scene::build(&fig.sh.snapshot(), None, &mut crate::scene::SceneCache::new());
        let a = &axes[0];
        let texts = decoration_texts(a, &dl.axes[0]);
        let r = a.rect;
        let xt = texts.iter().find(|t| t.kind == TextKind::XTickLabel).unwrap();
        assert!((xt.anchor[1] - (r.bottom() + 3.0 + 5.0 + 2.0)).abs() < 1e-9);
        let xl = texts.iter().find(|t| t.kind == TextKind::XLabel).unwrap();
        assert!((xl.anchor[1] - (r.bottom() + 3.0 + 5.0 + 16.31 + 2.0 + 3.0)).abs() < 1e-9);
        let yt = texts.iter().find(|t| t.kind == TextKind::YTickLabel).unwrap();
        assert!((yt.anchor[0] - (r.x - 3.0 - 5.0 - 4.0)).abs() < 1e-9);
    }
}
