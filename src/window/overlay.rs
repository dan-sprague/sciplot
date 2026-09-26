//! Window overlays drawn on top of the last frame without relayout: the rectangle-zoom shade,
//! the hover tooltip and its highlight ring. Also builds the interaction views of a frame and
//! runs hover picking on the frame's snapshot.

use super::interact::{AxisView, InteractState};
use crate::color::Color;
use crate::figure::FigState;
use crate::plots::pick::{Hover, PickCache, PickCtx};
use crate::scene::AxisFrame;
use crate::scene::drawlist::{
    Buf, DrawList, GlyphsPrim, Item, MarkersPrim, MeshPrim, MeshVertex, Prim, PrimColor, Rect, Space,
};
use crate::style::Marker;
use crate::text::{Font, RichText};
use std::borrow::Cow;

/// Makie's DataInspector `range` (units).
pub(crate) const PICK_RADIUS: f64 = 10.0;

/// Overlay draw order: above everything a figure draws (Makie draws the tooltip at depth 9e3).
mod z {
    pub const RECT_ZOOM: f32 = 9000.0;
    pub const RING: f32 = 9001.0;
    pub const TOOLTIP: f32 = 9002.0;
}

/// Tooltip style (Makie's `tooltip` recipe defaults).
const TOOLTIP_OFFSET: f64 = 10.0;
const TOOLTIP_TRIANGLE: f64 = 7.0;
/// Text padding, left, right, bottom, top.
const TOOLTIP_PADDING: [f64; 4] = [5.0, 5.0, 3.0, 3.0];
/// Makie's inspector indicator: red, linewidth 2.
const INDICATOR: Color = Color::rgb(1.0, 0.0, 0.0);
const INDICATOR_WIDTH: f64 = 2.0;

/// Interaction views of a built frame (axis rects, limits, scales, links as frame indices).
pub(crate) fn views(frames: &[AxisFrame], st: &FigState) -> Vec<AxisView> {
    let slot = |id| frames.iter().position(|f| f.id == id);
    frames
        .iter()
        .map(|f| {
            let ax = st.block(f.id).and_then(|b| b.as_axis());
            let links = |l: Option<&Vec<crate::figure::BlockId>>| -> Vec<usize> {
                l.map(|l| l.iter().filter_map(|id| slot(*id)).collect()).unwrap_or_default()
            };
            AxisView {
                rect: [f.rect.x, f.rect.y, f.rect.w, f.rect.h],
                limits: f.limits,
                xscale: f.attrs.xscale,
                yscale: f.attrs.yscale,
                xreversed: f.attrs.xreversed,
                yreversed: f.attrs.yreversed,
                xlinks: links(ax.map(|a| &a.xlinks)),
                ylinks: links(ax.map(|a| &a.ylinks)),
            }
        })
        .collect()
}

/// The nearest inspectable element under the cursor (figure units), if any.
pub(crate) fn pick(st: &FigState, frames: &[AxisFrame], cursor: [f64; 2], cache: &mut PickCache) -> Option<Hover> {
    let frame = frames.iter().rev().find(|f| f.rect.contains(cursor))?;
    let ax = st.block(frame.id).and_then(|b| b.as_axis())?;
    let g = st.theme.globals();
    let mut best: Option<Hover> = None;
    for pid in &ax.plots {
        let Some(p) = st.plot(*pid) else { continue };
        if !p.common.visible || !p.common.inspectable {
            continue;
        }
        let mut ctx = PickCtx {
            axis: frame,
            cursor,
            radius: PICK_RADIUS,
            uid: p.uid,
            data_rev: p.data_rev,
            theme: &st.theme,
            g: &g,
            cache: &mut *cache,
        };
        if let Some(h) = p.kind.imp().pick(&mut ctx)
            && best.as_ref().is_none_or(|b| h.dist <= b.dist)
        {
            best = Some(h);
        }
    }
    best
}

/// The frame's draw list plus the current overlays (borrowed unchanged when there are none).
pub(crate) fn compose<'a>(
    dl: &'a DrawList,
    st: &FigState,
    views: &[AxisView],
    ui: &InteractState,
    hover: Option<&Hover>,
) -> Cow<'a, DrawList> {
    let mut extra: Vec<(f32, Option<Rect>, Prim)> = Vec::new();
    if let Some((a, sel)) = ui.selection(views)
        && let Some(p) = rect_zoom_shade(&views[a], sel)
    {
        extra.push((z::RECT_ZOOM, None, p));
    }
    if let Some(h) = hover {
        let g = st.theme.globals();
        if let Some(d) = h.ring {
            let clip = views.iter().rev().find(|v| v.contains(h.anchor)).map(|v| {
                let [x, y, w, hh] = v.rect;
                Rect::new(x, y, w, hh)
            });
            extra.push((z::RING, clip, ring(h.anchor, d)));
        }
        for p in tooltip(h, dl.size, g.fontsize, g.textcolor) {
            extra.push((z::TOOLTIP, None, p));
        }
    }
    if extra.is_empty() {
        return Cow::Borrowed(dl);
    }
    let mut out = dl.clone();
    let first = out.items.iter().map(|i| i.seq + 1).max().unwrap_or(0);
    out.items.extend((first..).zip(extra).map(|(seq, (z, clip, prim))| Item {
        z,
        seq,
        clip,
        space: Space::Figure,
        prim,
    }));
    out.sort();
    Cow::Owned(out)
}

fn vert(p: [f64; 2], color: u32) -> MeshVertex {
    MeshVertex { pos: [p[0] as f32, p[1] as f32], color }
}

fn quad(out: &mut Vec<MeshVertex>, x0: f64, y0: f64, x1: f64, y1: f64, c: u32) {
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (a, b, cc, d) = ([x0, y0], [x1, y0], [x1, y1], [x0, y1]);
    out.extend([vert(a, c), vert(b, c), vert(cc, c), vert(a, c), vert(cc, c), vert(d, c)]);
}

/// Makie's rectangle-zoom visual: black at alpha 0.2 over the axis area *outside* the selection.
pub(crate) fn rect_zoom_shade(v: &AxisView, sel: [f64; 4]) -> Option<Prim> {
    let [ox, oy, w, h] = v.rect;
    let (ox1, oy1) = (ox + w, oy + h);
    let a = v.to_units(sel[0], sel[2])?;
    let b = v.to_units(sel[1], sel[3])?;
    let ix0 = a[0].min(b[0]).clamp(ox, ox1);
    let ix1 = a[0].max(b[0]).clamp(ox, ox1);
    let iy0 = a[1].min(b[1]).clamp(oy, oy1);
    let iy1 = a[1].max(b[1]).clamp(oy, oy1);
    let c = Color::rgba(0.0, 0.0, 0.0, 0.2).to_premul_u32();
    let mut verts = Vec::with_capacity(24);
    quad(&mut verts, ox, oy, ox1, iy0, c); // above
    quad(&mut verts, ox, iy1, ox1, oy1, c); // below
    quad(&mut verts, ox, iy0, ix0, iy1, c); // left
    quad(&mut verts, ix1, iy0, ox1, iy1, c); // right
    Some(Prim::Mesh(MeshPrim { verts: Buf::transient(verts) }))
}

/// A red ring (Makie's indicator style) around a hovered point.
fn ring(at: [f64; 2], diameter: f64) -> Prim {
    Prim::Markers(MarkersPrim {
        pos: Buf::transient(vec![[at[0] as f32, at[1] as f32]]),
        color: PrimColor::Uniform(Color::TRANSPARENT),
        size: diameter as f32,
        sizes: None,
        marker: Marker::FullCircle,
        stroke_color: INDICATOR,
        stroke_width: INDICATOR_WIDTH as f32,
        rotation: 0.0,
    })
}

/// Where the tooltip sits relative to the point (Makie's `update_tooltip_alignment!`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    Above,
    Below,
    Left,
    Right,
}

/// Makie's rule: above unless in the top quarter; right in the left quarter, left in the right.
pub(crate) fn placement(p: [f64; 2], size: [f64; 2]) -> Placement {
    let mut pl = if p[1] > 0.25 * size[1] { Placement::Above } else { Placement::Below };
    if p[0] < 0.25 * size[0] {
        pl = Placement::Right;
    }
    if p[0] > 0.75 * size[0] {
        pl = Placement::Left;
    }
    pl
}

/// Tooltip outline polygon (box plus the triangle pointing at `p`) and the box `[x0, y0, x1, y1]`.
pub(crate) fn tooltip_shape(p: [f64; 2], w: f64, h: f64, pl: Placement) -> (Vec<[f64; 2]>, [f64; 4]) {
    let (o, s) = (TOOLTIP_OFFSET, TOOLTIP_TRIANGLE);
    let hs = 0.5 * s;
    match pl {
        Placement::Above => {
            let (b, x0) = (p[1] - o - s, p[0] - 0.5 * w);
            let (t, x1) = (b - h, x0 + w);
            let poly = vec![[x0, b], [x0, t], [x1, t], [x1, b], [p[0] + hs, b], [p[0], p[1] - o], [p[0] - hs, b]];
            (poly, [x0, t, x1, b])
        }
        Placement::Below => {
            let (t, x0) = (p[1] + o + s, p[0] - 0.5 * w);
            let (b, x1) = (t + h, x0 + w);
            let poly = vec![[x0, t], [p[0] - hs, t], [p[0], p[1] + o], [p[0] + hs, t], [x1, t], [x1, b], [x0, b]];
            (poly, [x0, t, x1, b])
        }
        Placement::Right => {
            let (l, t) = (p[0] + o + s, p[1] - 0.5 * h);
            let (r, b) = (l + w, t + h);
            let poly = vec![[l, t], [r, t], [r, b], [l, b], [l, p[1] + hs], [p[0] + o, p[1]], [l, p[1] - hs]];
            (poly, [l, t, r, b])
        }
        Placement::Left => {
            let (r, t) = (p[0] - o - s, p[1] - 0.5 * h);
            let (l, b) = (r - w, t + h);
            let poly = vec![[l, t], [r, t], [r, p[1] - hs], [p[0] - o, p[1]], [r, p[1] + hs], [r, b], [l, b]];
            (poly, [l, t, r, b])
        }
    }
}

/// Makie's tooltip: white box with a 1 unit black outline and a triangle pointing at the anchor,
/// left-justified text with padding (5, 5, 3, 3).
pub(crate) fn tooltip(h: &Hover, fig: [f64; 2], fontsize: f64, textcolor: Color) -> Vec<Prim> {
    let l = crate::text::layout(&RichText::from(h.text.as_str()), fontsize, Font::Regular, textcolor);
    let [pl, pr, pb, pt] = TOOLTIP_PADDING;
    let (w, hh) = (l.width + pl + pr, l.height() + pb + pt);
    let place = placement(h.anchor, fig);
    let (poly, bx) = tooltip_shape(h.anchor, w, hh, place);

    // Fill: the box plus the triangle (vertices 4..=6 of every shape above, or 1..=3 for Below).
    let white = Color::rgb(1.0, 1.0, 1.0).to_premul_u32();
    let mut fill = Vec::with_capacity(9);
    quad(&mut fill, bx[0], bx[1], bx[2], bx[3], white);
    let tri = if place == Placement::Below { [poly[1], poly[2], poly[3]] } else { [poly[4], poly[5], poly[6]] };
    fill.extend(tri.iter().map(|p| vert(*p, white)));

    // Outline: one quad per edge, centered on it, with square caps so corners close.
    let black = Color::rgb(0.0, 0.0, 0.0).to_premul_u32();
    let hw = 0.5;
    let mut stroke = Vec::with_capacity(poly.len() * 6);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy);
        if len == 0.0 {
            continue;
        }
        let (ux, uy) = (dx / len * hw, dy / len * hw);
        let (nx, ny) = (-uy, ux);
        let p0 = [a[0] - ux + nx, a[1] - uy + ny];
        let p1 = [b[0] + ux + nx, b[1] + uy + ny];
        let p2 = [b[0] + ux - nx, b[1] + uy - ny];
        let p3 = [a[0] - ux - nx, a[1] - uy - ny];
        stroke.extend([
            vert(p0, black),
            vert(p1, black),
            vert(p2, black),
            vert(p0, black),
            vert(p2, black),
            vert(p3, black),
        ]);
    }

    let glyphs = crate::text::place(&l, [bx[0] + pl, bx[1] + pt], (0.0, 1.0), 0.0);
    vec![
        Prim::Mesh(MeshPrim { verts: Buf::transient(fill) }),
        Prim::Mesh(MeshPrim { verts: Buf::transient(stroke) }),
        Prim::Glyphs(GlyphsPrim { glyphs }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_picks_the_nearest_scatter_point() {
        use crate::prelude::*;
        for (xscale, yrev) in [(Scale::Identity, false), (Scale::Log10, true)] {
            let fig = Figure::new();
            let ax = Axis::new(fig.at(1, 1)).xscale(xscale).yreversed(yrev);
            ax.scatter([1.0, 10.0, 100.0, 1000.0], [1.0, 2.0, -3.5e-7, 4.0]);
            let st = fig.sh.snapshot();
            let (_, frames) = crate::scene::build(&st, None, &mut crate::scene::SceneCache::new());
            let mut cache = PickCache::default();
            let p = frames[0].to_units(10.0, 2.0).unwrap();
            let h = pick(&st, &frames, [p[0] + 3.0, p[1] - 4.0], &mut cache).unwrap();
            assert_eq!(h.text, "x: 10\ny: 2");
            assert!((h.dist - 5.0).abs() < 1e-9);
            assert_eq!(h.anchor, p);
            let q = frames[0].to_units(100.0, -3.5e-7).unwrap();
            assert_eq!(pick(&st, &frames, q, &mut cache).unwrap().text, "x: 100\ny: -3.5e-07");
            // Out of range (10 units), and outside every axis.
            assert!(pick(&st, &frames, [p[0] + 8.0, p[1] + 8.0], &mut cache).is_none());
            assert!(pick(&st, &frames, [1.0, 1.0], &mut cache).is_none());
        }
    }

    #[test]
    fn placement_follows_makie() {
        let fig = [600.0, 450.0];
        assert_eq!(placement([300.0, 300.0], fig), Placement::Above);
        assert_eq!(placement([300.0, 50.0], fig), Placement::Below);
        assert_eq!(placement([100.0, 300.0], fig), Placement::Right);
        assert_eq!(placement([500.0, 50.0], fig), Placement::Left);
    }

    #[test]
    fn tooltip_triangle_tip_is_offset_from_the_point() {
        let (poly, bx) = tooltip_shape([300.0, 300.0], 80.0, 40.0, Placement::Above);
        assert!(poly.contains(&[300.0, 290.0]));
        assert_eq!(bx, [260.0, 243.0, 340.0, 283.0]);
    }

    #[test]
    fn rect_zoom_shade_covers_outside_only() {
        let v = AxisView::new([100.0, 50.0, 400.0, 300.0], [0.0, 10.0, 0.0, 10.0]);
        let Some(Prim::Mesh(m)) = rect_zoom_shade(&v, [2.0, 6.0, 5.0, 10.0]) else { panic!() };
        // Triangle areas add up to the outer area minus the selection.
        let area: f64 = m
            .verts
            .data
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos].map(|p| [p[0] as f64, p[1] as f64]);
                0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs()
            })
            .sum();
        let sel = (0.4 * 400.0) * (0.5 * 300.0);
        assert!((area - (400.0 * 300.0 - sel)).abs() < 1e-6, "{area}");
    }
}
