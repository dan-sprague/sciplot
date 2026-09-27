//! Shared bar geometry for `barplot` and `hist` (Makie's barplot recipe).
//!
//! Provenance: bar layout adapted from Makie 0.24.14 `src/basic_recipes/barplot.jl` (automatic
//! `width`, `compute_x_and_width`, `scale_width`, `shift_dodge`, `bar_rectangle`; stacking
//! simplified from `stack_grouped_from_to`). Bar hover follows
//! `show_data(::DataInspector, ::BarPlot, idx)` in `src/interaction/inspector.jl`; outlines follow
//! CairoMakie 0.15.14 `src/overrides.jl` (`draw_poly`). MIT licensed; see THIRD_PARTY_NOTICES.md.

use super::pick::{Hover, PickCtx, point_text};
use crate::color::Color;
use crate::scene::drawlist::{Buf, LinesPrim, MeshPrim, MeshVertex, Prim, PrimColor};
use crate::scene::{AxisFrame, PlotCtx};
use crate::style::{Direction, JoinStyle, LineCap};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
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

impl Bar {
    /// `[x0, x1, y0, y1]` in data space (bars of `Direction::X` run along x).
    fn oriented(&self, dir: Direction) -> [f64; 4] {
        match dir {
            Direction::Y => [self.x0, self.x1, self.y0, self.y1],
            Direction::X => [self.y0, self.y1, self.x0, self.x1],
        }
    }

    /// The corners `[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]` in the axis' local coordinates, or
    /// `None` if one is outside the scales' domain.
    fn local_corners(&self, dir: Direction, a: &AxisFrame) -> Option<[[f32; 2]; 4]> {
        let [x0, x1, y0, y1] = self.oriented(dir);
        let (xs, ys) = (a.attrs.xscale, a.attrs.yscale);
        let p = |x: f64, y: f64| a.rebase.to_local(xs.forward(x), ys.forward(y));
        let q = [p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)];
        q.iter().all(|v| v[0].is_finite() && v[1].is_finite()).then_some(q)
    }
}

/// Emits bars as a data-space triangle mesh (MSAA antialiases the edges).
pub(crate) fn emit_bars(ctx: &mut PlotCtx<'_>, part: u8, bars: &[Bar], colors: &[Color], dir: Direction) {
    // The mesh changes with the geometry and colors, not only with the data.
    let mut h = DefaultHasher::new();
    ctx.conv_key(part).hash(&mut h);
    (dir == Direction::X).hash(&mut h);
    for b in bars {
        [b.x0, b.x1, b.y0, b.y1].map(f64::to_bits).hash(&mut h);
    }
    for c in colors {
        c.to_premul_u32().hash(&mut h);
    }
    let key = h.finish();
    let a = ctx.axis;
    let bars2 = bars.to_vec();
    let colors2 = colors.to_vec();
    let verts = ctx.cache.memo(ctx.uid, part, key, move || {
        let mut v = Vec::with_capacity(bars2.len() * 6);
        for (i, b) in bars2.iter().enumerate() {
            let color = colors2[i.min(colors2.len().saturating_sub(1))].to_premul_u32();
            let Some(q) = b.local_corners(dir, a) else { continue };
            let q = q.map(|pos| MeshVertex { pos, color });
            v.extend_from_slice(&[q[0], q[1], q[3], q[1], q[2], q[3]]);
        }
        v
    });
    let buf = ctx.keyed_buf(part, key, verts);
    ctx.push_data(Prim::Mesh(MeshPrim { verts: buf }));
}

/// Makie's poly stroke of every bar: the rectangle's closed outline, centered on its edges, with
/// miter joins (CairoMakie's `fill_preserve` + `stroke` per rectangle). Drawn after all fills.
pub(crate) fn emit_bar_strokes(ctx: &mut PlotCtx<'_>, bars: &[Bar], color: Color, width: f64, dir: Direction) {
    if width.is_nan() || width <= 0.0 || color.a <= 0.0 {
        return;
    }
    for b in bars {
        let Some(q) = b.local_corners(dir, ctx.axis) else { continue };
        // A flat bar (an empty histogram bin) strokes as one line with flat ends, as Cairo does.
        let (pts, closed) = match (q[0] == q[1], q[0] == q[3]) {
            (true, true) => continue,
            (false, true) => (vec![q[0], q[1]], false),
            (true, false) => (vec![q[0], q[3]], false),
            (false, false) => (vec![q[0], q[1], q[2], q[3], q[0]], true),
        };
        ctx.push_data(Prim::Lines(LinesPrim {
            pts: Buf::transient(pts),
            color: PrimColor::Uniform(color),
            width: width as f32,
            pattern: None,
            cap: LineCap::Butt,
            join: JoinStyle::Miter,
            miter_limit: std::f32::consts::FRAC_PI_3,
            segments: false,
            closed,
            append: false,
        }));
    }
}

/// Makie's barplot inspection (`show_data(::DataInspector, ::BarPlot, idx)`): the topmost bar
/// under the cursor, labelled with `point(i)` (the bar's input position and height, swapped for
/// horizontal bars) and outlined.
pub(crate) fn pick_bar(
    ctx: &PickCtx<'_>,
    bars: &[Bar],
    dir: Direction,
    point: impl Fn(usize) -> Option<[f64; 2]>,
) -> Option<Hover> {
    let [cx, cy] = ctx.cursor_data()?;
    let (i, b) = bars.iter().enumerate().rev().find(|(_, b)| {
        let [x0, x1, y0, y1] = b.oriented(dir);
        (x0..=x1).contains(&cx) && (y0..=y1).contains(&cy)
    })?;
    let [x0, x1, y0, y1] = b.oriented(dir);
    let outline = ctx
        .axis
        .to_units(x0, y0)
        .zip(ctx.axis.to_units(x1, y1))
        .map(|(p, q)| [p[0].min(q[0]), p[1].min(q[1]), (p[0] - q[0]).abs(), (p[1] - q[1]).abs()]);
    let [x, y] = point(i)?;
    let [x, y] = if dir == Direction::X { [y, x] } else { [x, y] };
    Some(Hover {
        // Anything else within the pick radius (drawn over the bars) wins.
        dist: ctx.radius,
        anchor: ctx.cursor,
        text: point_text(x, y),
        ring: None,
        outline,
    })
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
    use crate::prelude::*;
    use crate::scene::drawlist::DrawList;
    use crate::scene::{SceneCache, build};

    fn lines(dl: &DrawList) -> Vec<&LinesPrim> {
        dl.items
            .iter()
            .filter_map(|i| match &i.prim {
                Prim::Lines(l) => Some(l),
                _ => None,
            })
            .collect()
    }

    fn mesh_colors(dl: &DrawList) -> Vec<u32> {
        dl.items
            .iter()
            .filter_map(|i| match &i.prim {
                Prim::Mesh(m) => Some(m.verts.data.iter().map(|v| v.color).collect::<Vec<_>>()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// The hover result of the figure's first plot at `data` (a data-space point).
    fn hover_at(fig: &Figure, data: [f64; 2]) -> Option<Hover> {
        let st = fig.sh.snapshot();
        let (_, axes) = build(&st, None, &mut SceneCache::new());
        let a = &axes[0];
        let g = st.theme.globals();
        let p = st.plots[0].as_ref().unwrap();
        let mut cache = crate::plots::pick::PickCache::default();
        let mut ctx = PickCtx {
            axis: a,
            cursor: a.to_units(data[0], data[1]).unwrap(),
            radius: 10.0,
            uid: p.uid,
            data_rev: p.data_rev,
            theme: &st.theme,
            g: &g,
            cache: &mut cache,
        };
        p.kind.imp().pick(&mut ctx)
    }

    #[test]
    fn bar_strokes_are_closed_outlines() {
        let b = barplot([1.0, 2.0, 3.0], [2.0, 0.0, -1.0]);
        let fig = b.figure();
        let dl = build(&fig.sh.snapshot(), None, &mut SceneCache::new()).0;
        assert!(lines(&dl).is_empty(), "no stroke by default (strokewidth 0)");
        b.strokewidth(3).strokecolor((BLUE, 0.8)).alpha(0.5);
        let dl = build(&fig.sh.snapshot(), None, &mut SceneCache::new()).0;
        let ls = lines(&dl);
        assert_eq!(ls.len(), 3, "one outline per bar");
        for l in &ls {
            assert_eq!(l.width, 3.0);
            assert_eq!(l.join, JoinStyle::Miter);
            assert!(matches!(l.color, PrimColor::Uniform(c) if (c.a - 0.4).abs() < 1e-6 && c.b == 1.0));
        }
        // Rectangles are closed loops; the flat bar is a single line with flat ends.
        assert!(ls[0].closed && ls[0].pts.len() == 5 && ls[0].pts.data[0] == ls[0].pts.data[4]);
        assert!(!ls[1].closed && ls[1].pts.len() == 2);
        assert!(ls[2].closed);
        // Strokes are drawn after (over) the fills.
        let first_line = dl.items.iter().position(|i| matches!(i.prim, Prim::Lines(_))).unwrap();
        let mesh = dl.items.iter().position(|i| matches!(i.prim, Prim::Mesh(_))).unwrap();
        assert!(mesh < first_line);
    }

    #[test]
    fn hist_strokes_every_bin() {
        let h = hist([1.0, 2.0, 2.5, 4.0]).bins(3).strokewidth(1);
        let dl = build(&h.figure().sh.snapshot(), None, &mut SceneCache::new()).0;
        assert_eq!(lines(&dl).len(), 3);
    }

    #[test]
    fn bar_mesh_follows_style_changes() {
        // Attribute changes don't bump the data revision; the cached mesh must still update.
        let b = barplot([1.0, 2.0], [1.0, 2.0]).color(RED);
        let fig = b.figure();
        let mut cache = SceneCache::new();
        let red = mesh_colors(&build(&fig.sh.snapshot(), None, &mut cache).0);
        b.color(BLUE);
        let blue = mesh_colors(&build(&fig.sh.snapshot(), None, &mut cache).0);
        assert!(red.iter().all(|c| *c == RED.to_premul_u32()) && !red.is_empty());
        assert!(blue.iter().all(|c| *c == BLUE.to_premul_u32()) && blue.len() == red.len());
    }

    #[test]
    fn pick_bar_under_cursor() {
        let b = barplot([1.0, 2.0, 3.0], [1.0, 2.5, 3.0]);
        b.axis().limits(0.0, 4.0, 0.0, 4.0);
        let fig = b.figure();
        let h = hover_at(&fig, [2.1, 1.0]).expect("inside bar 2");
        assert_eq!(h.text, "x: 2\ny: 2.5");
        // The outline is bar 2 (x 1.6..2.4, y 0..2.5) in figure units.
        let st = fig.sh.snapshot();
        let (_, axes) = build(&st, None, &mut SceneCache::new());
        let (p, q) = (axes[0].to_units(1.6, 2.5).unwrap(), axes[0].to_units(2.4, 0.0).unwrap());
        let o = h.outline.unwrap();
        let want = [p[0], p[1], q[0] - p[0], q[1] - p[1]];
        assert!(o.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-9), "{o:?} vs {want:?}");
        assert!(hover_at(&fig, [2.0, 3.0]).is_none(), "above the bar");
        assert!(hover_at(&fig, [1.5, 0.5]).is_none(), "in the gap");
        // Horizontal bars report (height, position) like Makie.
        b.direction(Direction::X);
        b.axis().limits(0.0, 4.0, 0.0, 4.0);
        assert_eq!(hover_at(&fig, [1.0, 2.1]).unwrap().text, "x: 2.5\ny: 2");
    }

    #[test]
    fn pick_hist_bin() {
        let h = hist([0.0, 1.0, 1.5, 3.0]).bins(Bins::Edges(vec![0.0, 1.0, 2.0, 3.0, 4.0]));
        h.axis().limits(0.0, 4.0, 0.0, 3.0);
        let hover = hover_at(&h.figure(), [1.2, 1.0]).expect("inside bin [1, 2)");
        assert_eq!(hover.text, "x: 1.5\ny: 2");
    }

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
