//! Shared bar geometry for `barplot` and `hist` (Makie's barplot recipe).

use crate::color::Color;
use crate::scene::PlotCtx;
use crate::scene::drawlist::{MeshPrim, MeshVertex, Prim};
use crate::style::Direction;
use std::sync::Arc;

/// One bar in data space: spans `x0..x1` along the category axis and `y0..y1` along the value axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bar {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}

/// Makie's default bar width: the minimum spacing of the unique finite x values (1 for one bar).
pub(crate) fn auto_width(x: &[f64]) -> f64 {
    let mut u: Vec<f64> = x.iter().copied().filter(|v| v.is_finite()).collect();
    u.sort_by(f64::total_cmp);
    u.dedup();
    let w = u.windows(2).map(|w| w[1] - w[0]).fold(f64::INFINITY, f64::min);
    if w.is_finite() { w } else { 1.0 }
}

/// Parameters of the barplot recipe.
pub(crate) struct BarLayout<'a> {
    pub width: Option<f64>,
    pub gap: f64,
    pub dodge: Option<&'a [usize]>,
    pub dodge_gap: f64,
    pub stack: Option<&'a [usize]>,
    pub fillto: f64,
    pub offset: f64,
}

/// Bars for `x`/`h` following Makie's barplot recipe (width, gap, dodge, stack, fillto, offset).
pub(crate) fn layout_bars(x: &[f64], h: &[f64], p: &BarLayout) -> Vec<Bar> {
    let w = p.width.unwrap_or_else(|| auto_width(x));
    let w_eff = w * (1.0 - p.gap);
    let n_dodge = p.dodge.map_or(1, |d| d.iter().copied().max().unwrap_or(1).max(1));
    let dw = (1.0 - (n_dodge as f64 - 1.0) * p.dodge_gap) / n_dodge as f64;
    // Stacking: running positive/negative sums per (x, dodge) group.
    let mut sums: std::collections::HashMap<(u64, usize), (f64, f64)> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(x.len());
    for i in 0..x.len().min(h.len()) {
        let di = p.dodge.map_or(1, |d| d.get(i).copied().unwrap_or(1).max(1));
        let (xc, bw) = if n_dodge > 1 {
            (x[i] + w_eff * ((dw - 1.0) / 2.0 + (di as f64 - 1.0) * (dw + p.dodge_gap)), w_eff * dw)
        } else {
            (x[i], w_eff)
        };
        let (from, to) = if p.stack.is_some() {
            let e = sums.entry((xc.to_bits(), di)).or_insert((0.0, 0.0));
            let hv = h[i];
            let base = if hv >= 0.0 { &mut e.0 } else { &mut e.1 };
            let b = *base;
            *base += hv;
            (p.fillto + b + p.offset, p.fillto + b + hv + p.offset)
        } else {
            (p.fillto + p.offset, h[i] + p.offset)
        };
        out.push(Bar { x0: xc - 0.5 * bw.abs(), x1: xc + 0.5 * bw.abs(), y0: from.min(to), y1: from.max(to) });
    }
    out
}

/// Emits bars as a data-space triangle mesh (MSAA antialiases the edges).
pub(crate) fn emit_bars(ctx: &mut PlotCtx<'_>, part: u8, bars: &[Bar], colors: &[Color], dir: Direction) {
    let key = ctx.conv_key(part);
    let a = ctx.axis;
    let bars2 = bars.to_vec();
    let colors2 = colors.to_vec();
    let verts = ctx.cache.memo(ctx.uid, part, key, move || {
        let mut v = Vec::with_capacity(bars2.len() * 6);
        for (i, b) in bars2.iter().enumerate() {
            let c = colors2[i.min(colors2.len().saturating_sub(1))].to_premul_u32();
            let (x0, x1, y0, y1) = match dir {
                Direction::Y => (b.x0, b.x1, b.y0, b.y1),
                Direction::X => (b.y0, b.y1, b.x0, b.x1),
            };
            let (xs, ys) = (a.attrs.xscale, a.attrs.yscale);
            let p = |x: f64, y: f64| {
                let (sx, sy) = (xs.forward(x), ys.forward(y));
                MeshVertex { pos: a.rebase.to_local(sx, sy), color: c }
            };
            let q = [p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1)];
            if q.iter().any(|v| !(v.pos[0].is_finite() && v.pos[1].is_finite())) {
                continue;
            }
            v.extend_from_slice(&[q[0], q[1], q[2], q[1], q[3], q[2]]);
        }
        v
    });
    let buf = ctx.keyed_buf(part, key, verts);
    ctx.push_data(Prim::Mesh(MeshPrim { verts: buf }));
}

/// Scaled-space bounds of bars.
pub(crate) fn bars_bounds(
    bars: &[Bar],
    dir: Direction,
    xs: crate::transform::Scale,
    ys: crate::transform::Scale,
) -> Option<[f64; 4]> {
    let pts: Vec<[f64; 2]> = bars
        .iter()
        .flat_map(|b| match dir {
            Direction::Y => [[b.x0, b.y0], [b.x1, b.y1]],
            Direction::X => [[b.y0, b.x0], [b.y1, b.x1]],
        })
        .collect();
    super::point_bounds(&pts, xs, ys)
}

pub(crate) fn arc_colors(c: Color, n: usize) -> Arc<Vec<Color>> {
    Arc::new(vec![c; n.max(1)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lay(x: &[f64], h: &[f64]) -> Vec<Bar> {
        layout_bars(
            x,
            h,
            &BarLayout { width: None, gap: 0.2, dodge: None, dodge_gap: 0.03, stack: None, fillto: 0.0, offset: 0.0 },
        )
    }

    #[test]
    fn default_width_and_gap() {
        let b = lay(&[1.0, 2.0, 3.0], &[1.0, -2.0, 3.0]);
        assert!((b[0].x0 - 0.6).abs() < 1e-12 && (b[0].x1 - 1.4).abs() < 1e-12);
        assert_eq!((b[1].y0, b[1].y1), (-2.0, 0.0));
    }

    #[test]
    fn dodge_and_stack() {
        let d = [1usize, 2];
        let b = layout_bars(
            &[1.0, 1.0],
            &[1.0, 2.0],
            &BarLayout {
                width: None,
                gap: 0.2,
                dodge: Some(&d),
                dodge_gap: 0.03,
                stack: None,
                fillto: 0.0,
                offset: 0.0,
            },
        );
        assert!(b[0].x1 < b[1].x0, "dodged bars don't overlap");
        let s = [1usize, 2];
        let b = layout_bars(
            &[1.0, 1.0],
            &[1.0, 2.0],
            &BarLayout {
                width: None,
                gap: 0.2,
                dodge: None,
                dodge_gap: 0.03,
                stack: Some(&s),
                fillto: 0.0,
                offset: 0.0,
            },
        );
        assert_eq!((b[1].y0, b[1].y1), (1.0, 3.0));
    }
}
