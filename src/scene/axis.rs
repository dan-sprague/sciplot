//! Axis limits, protrusions and decorations (background, grid, ticks, spines, labels, title).

use super::AxisFrame;
use super::drawlist::{AxisXform, Emitter, GlyphsPrim, Prim, Rect, RectPrim, Space};
use crate::blocks::Block;
use crate::blocks::axis::AxisResolved;
use crate::figure::{BlockId, FigState};
use crate::layout::Protrusion;
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
            // Union with linked axes before margins.
            let mut bx = raw[i].0;
            let mut by = raw[i].1;
            let mut tight = raw[i].2;
            for l in &ax.xlinks {
                if let Some(j) = index_of(l) {
                    bx = union(bx, raw[j].0);
                    tight |= raw[j].2;
                }
            }
            for l in &ax.ylinks {
                if let Some(j) = index_of(l) {
                    by = union(by, raw[j].1);
                    tight |= raw[j].2;
                }
            }
            let (mx, my) = if tight { ([0.0; 2], [0.0; 2]) } else { (r.xautolimitmargin, r.yautolimitmargin) };
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

/// Height of one line of text at `size` (Makie: ascender - descender = 1.165 em for TeX Gyre Heros).
pub(crate) fn line_height(size: f64) -> f64 {
    crate::text::line_height(size)
}

fn max_label_width(t: &Ticks, size: f64, font: crate::text::Font) -> f64 {
    t.labels.iter().map(|l| crate::text::measure(l, size, font).width).fold(0.0, f64::max)
}

/// Makie's Axis protrusions.
pub(crate) fn protrusion(a: &AxisResolved, xt: &Ticks, yt: &Ticks) -> Protrusion {
    let xtick_out = if a.xticksvisible { a.xticksize * (1.0 - a.xtickalign) } else { 0.0 };
    let ytick_out = if a.yticksvisible { a.yticksize * (1.0 - a.ytickalign) } else { 0.0 };
    let xticklabel_h = match a.xticklabelspace {
        Some(s) => s,
        None if a.xticklabelsvisible && !xt.labels.is_empty() => line_height(a.xticklabelsize),
        None => 0.0,
    };
    let yticklabel_w = match a.yticklabelspace {
        Some(s) => s,
        None if a.yticklabelsvisible => max_label_width(yt, a.yticklabelsize, a.yticklabelfont),
        None => 0.0,
    };
    let mut bottom = xtick_out + xticklabel_h + if xticklabel_h > 0.0 { a.xticklabelpad } else { 0.0 };
    if a.xlabelvisible && !a.xlabel.is_empty() {
        bottom += a.xlabelpadding + line_height(a.xlabelsize);
    }
    let mut left = ytick_out + yticklabel_w + if yticklabel_w > 0.0 { a.yticklabelpad } else { 0.0 };
    if a.ylabelvisible && !a.ylabel.is_empty() {
        left += a.ylabelpadding + line_height(a.ylabelsize);
    }
    let mut top = 0.0;
    if a.titlevisible && !a.title.is_empty() {
        top += a.titlegap + line_height(a.titlesize);
        if !a.subtitle.is_empty() {
            top += a.subtitlegap + line_height(a.subtitlesize);
        }
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

fn inside(v: f64, lo: f64, hi: f64) -> bool {
    let eps = 1e-9 * (hi - lo).abs();
    v >= lo.min(hi) - eps && v <= lo.max(hi) + eps
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
    if at.xminorgridvisible {
        for &v in &xminor {
            let x = ux(a, xf, v);
            grid.push(RectPrim {
                rect: Rect::new(x - 0.5 * at.xminorgridwidth, r.y, at.xminorgridwidth, r.h),
                color: at.xminorgridcolor,
                snap: true,
            });
        }
    }
    if at.yminorgridvisible {
        for &v in &yminor {
            let y = uy(a, xf, v);
            grid.push(RectPrim {
                rect: Rect::new(r.x, y - 0.5 * at.yminorgridwidth, r.w, at.yminorgridwidth),
                color: at.yminorgridcolor,
                snap: true,
            });
        }
    }
    if at.xgridvisible {
        for &v in &xs {
            let x = ux(a, xf, v);
            grid.push(RectPrim {
                rect: Rect::new(x - 0.5 * at.xgridwidth, r.y, at.xgridwidth, r.h),
                color: at.xgridcolor,
                snap: true,
            });
        }
    }
    if at.ygridvisible {
        for &v in &ys {
            let y = uy(a, xf, v);
            grid.push(RectPrim {
                rect: Rect::new(r.x, y - 0.5 * at.ygridwidth, r.w, at.ygridwidth),
                color: at.ygridcolor,
                snap: true,
            });
        }
    }
    if !grid.is_empty() {
        em.push(z::GRID, Some(r), Space::Figure, rects(grid));
    }

    // Ticks (outside by default: tickalign 0).
    let mut ticks = Vec::new();
    let xtick = |v: f64, size: f64, width: f64, color, out: &mut Vec<RectPrim>| {
        let x = ux(a, xf, v);
        let y0 = r.bottom() - size * at.xtickalign;
        out.push(RectPrim { rect: Rect::new(x - 0.5 * width, y0, width, size), color, snap: true });
    };
    if at.xticksvisible {
        for &v in &xs {
            xtick(v, at.xticksize, at.xtickwidth, at.xtickcolor, &mut ticks);
        }
    }
    if at.xminorticksvisible {
        for &v in &xminor {
            xtick(v, at.xminorticksize, at.xminortickwidth, at.xminortickcolor, &mut ticks);
        }
    }
    let ytick = |v: f64, size: f64, width: f64, color, out: &mut Vec<RectPrim>| {
        let y = uy(a, xf, v);
        let x0 = r.x - size * (1.0 - at.ytickalign);
        out.push(RectPrim { rect: Rect::new(x0, y - 0.5 * width, size, width), color, snap: true });
    };
    if at.yticksvisible {
        for &v in &ys {
            ytick(v, at.yticksize, at.ytickwidth, at.ytickcolor, &mut ticks);
        }
    }
    if at.yminorticksvisible {
        for &v in &yminor {
            ytick(v, at.yminorticksize, at.yminortickwidth, at.yminortickcolor, &mut ticks);
        }
    }
    if !ticks.is_empty() {
        em.push(z::TICKS, None, Space::Figure, rects(ticks));
    }

    // Spines, centered on the axis boundary.
    let sw = at.spinewidth;
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
    let mut glyphs = Vec::new();
    let xtick_out = if at.xticksvisible { at.xticksize * (1.0 - at.xtickalign) } else { 0.0 };
    let ytick_out = if at.yticksvisible { at.yticksize * (1.0 - at.ytickalign) } else { 0.0 };
    let mut xlabel_top = r.bottom() + xtick_out;
    if at.xticklabelsvisible {
        for (v, label) in a.xticks.values.iter().zip(&a.xticks.labels) {
            if !inside(*v, a.limits[0], a.limits[1]) {
                continue;
            }
            let l = crate::text::layout(label, at.xticklabelsize, at.xticklabelfont, at.xticklabelcolor);
            glyphs.extend(crate::text::place(
                &l,
                [ux(a, xf, *v), r.bottom() + xtick_out + at.xticklabelpad],
                (0.5, 1.0),
                0.0,
            ));
        }
        let h =
            at.xticklabelspace.unwrap_or(if a.xticks.labels.is_empty() { 0.0 } else { line_height(at.xticklabelsize) });
        if h > 0.0 {
            xlabel_top += at.xticklabelpad + h;
        }
    }
    if at.xlabelvisible && !at.xlabel.is_empty() {
        let l = crate::text::layout(&at.xlabel, at.xlabelsize, at.xlabelfont, at.xlabelcolor);
        glyphs.extend(crate::text::place(&l, [r.x + 0.5 * r.w, xlabel_top + at.xlabelpadding], (0.5, 1.0), 0.0));
    }
    let mut ylabel_right = r.x - ytick_out;
    if at.yticklabelsvisible {
        let mut maxw: f64 = 0.0;
        for (v, label) in a.yticks.values.iter().zip(&a.yticks.labels) {
            let l = crate::text::layout(label, at.yticklabelsize, at.yticklabelfont, at.yticklabelcolor);
            maxw = maxw.max(l.width);
            if !inside(*v, a.limits[2], a.limits[3]) {
                continue;
            }
            glyphs.extend(crate::text::place(&l, [r.x - ytick_out - at.yticklabelpad, uy(a, xf, *v)], (1.0, 0.5), 0.0));
        }
        let w = at.yticklabelspace.unwrap_or(maxw);
        if w > 0.0 {
            ylabel_right -= at.yticklabelpad + w;
        }
    }
    if at.ylabelvisible && !at.ylabel.is_empty() {
        let l = crate::text::layout(&at.ylabel, at.ylabelsize, at.ylabelfont, at.ylabelcolor);
        // Rotated 90° counter-clockwise; the text's bottom faces the axis.
        glyphs.extend(crate::text::place(
            &l,
            [ylabel_right - at.ylabelpadding, r.y + 0.5 * r.h],
            (0.5, 0.0),
            std::f64::consts::FRAC_PI_2,
        ));
    }
    if at.titlevisible && !at.title.is_empty() {
        let l = crate::text::layout(&at.title, at.titlesize, at.titlefont, at.titlecolor);
        let f = at.titlealign.frac();
        let mut y = r.y - at.titlegap;
        if !at.subtitle.is_empty() {
            let s = crate::text::layout(&at.subtitle, at.subtitlesize, crate::text::Font::Regular, at.subtitlecolor);
            glyphs.extend(crate::text::place(&s, [r.x + f * r.w, y], (f, 0.0), 0.0));
            y -= line_height(at.subtitlesize) + at.subtitlegap;
        }
        glyphs.extend(crate::text::place(&l, [r.x + f * r.w, y], (f, 0.0), 0.0));
    }
    if !glyphs.is_empty() {
        em.push(z::TEXT, None, Space::Figure, Prim::Glyphs(GlyphsPrim { glyphs }));
    }
}

#[allow(dead_code)]
pub(crate) fn is_axis(b: &Block) -> bool {
    matches!(b, Block::Axis(_))
}
