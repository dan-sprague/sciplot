//! 3D items (`Prim::Lines3d` / `Markers3d` / `Mesh3d`) for the SVG backend: painter's order.
//!
//! The items of one Axis3 (one depth group) are projected to figure units on the CPU in f64 and
//! split into elements: triangles (colored per vertex with Makie's FastShading, as CairoMakie
//! does; the SVG writer fills each triangle with the mean color), line segments (the color at
//! their midpoint, round caps so polylines join round) and markers. The elements are sorted back
//! to front by their mean depth (CairoMakie's `average_z`) and written as 2D primitives, merging
//! runs of equal style. Content outside the limits box is dropped (segments are cut at the box;
//! triangles and markers with a vertex or centre outside are left out, like CairoMakie).
//!
//! Triangles are grown by [`SEAM`] units to hide antialiasing seams between them. This
//! approximates the GPU's per-pixel depth test: intersecting triangles and segments
//! crossing a surface are ordered as a whole, not per pixel.

use super::field::Mapper;
use super::num::unpremul_u32;
use crate::color::Color;
use crate::scene::drawlist::{Buf, Item, LinesPrim, MarkersPrim, MeshPrim, MeshVertex, Prim, PrimColor, View3d};
use crate::style::{JoinStyle, LineCap, Marker};

/// The depth group of a 3D primitive (`None` for 2D primitives).
pub(super) fn group(p: &Prim) -> Option<u64> {
    match p {
        Prim::Lines3d(l) => Some(l.view.group),
        Prim::Markers3d(m) => Some(m.view.group),
        Prim::Mesh3d(m) => Some(m.view.group),
        _ => None,
    }
}

/// How far (units) triangle edges are pushed out to hide antialiasing seams.
const SEAM: f64 = 0.35;

#[derive(Clone, Copy, PartialEq)]
struct MarkStyle {
    marker: Marker,
    size: f32,
    stroke_color: Color,
    stroke_width: f32,
}

enum El {
    Tri([[f64; 2]; 3], [Color; 3]),
    Seg([[f64; 2]; 2], Color, f32),
    Mark([f64; 2], Color, MarkStyle),
}

/// Local point -> (figure units, NDC depth), `None` behind the camera.
fn project(v: &View3d, p: [f32; 3]) -> Option<([f64; 2], f64)> {
    let q = crate::scene::axis3::camera::apply(&v.mvp, p.map(f64::from));
    if !(q[3] > 0.0) || !q.iter().all(|c| c.is_finite()) {
        return None;
    }
    let (x, y, z) = (q[0] / q[3], q[1] / q[3], q[2] / q[3]);
    let a = v.area;
    Some(([a.x + 0.5 * (x + 1.0) * a.w, a.y + 0.5 * (1.0 - y) * a.h], z))
}

fn in_box(v: &View3d, p: [f32; 3]) -> bool {
    match v.clip {
        Some([lo, hi]) => (0..3).all(|i| p[i] >= lo[i] && p[i] <= hi[i]),
        None => true,
    }
}

/// Cuts the segment `a`–`b` at the view's clip box (Liang–Barsky); `None` if it misses it.
fn clip_segment(v: &View3d, a: [f32; 3], b: [f32; 3]) -> Option<(f32, f32)> {
    let Some([lo, hi]) = v.clip else { return Some((0.0, 1.0)) };
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for i in 0..3 {
        let d = b[i] - a[i];
        if d == 0.0 {
            if a[i] < lo[i] || a[i] > hi[i] {
                return None;
            }
            continue;
        }
        let (ta, tb) = ((lo[i] - a[i]) / d, (hi[i] - a[i]) / d);
        t0 = t0.max(ta.min(tb));
        t1 = t1.min(ta.max(tb));
    }
    (t0 <= t1).then_some((t0, t1))
}

/// Straight colors per element (or per vertex) of a primitive color.
struct Colors<'a> {
    color: &'a PrimColor,
    mapper: Option<Mapper>,
}

impl<'a> Colors<'a> {
    fn new(color: &'a PrimColor) -> Colors<'a> {
        let mapper = match color {
            PrimColor::Values(_, m) => Some(Mapper::new(m)),
            _ => None,
        };
        Colors { color, mapper }
    }

    fn at(&self, i: usize) -> Color {
        match self.color {
            PrimColor::Uniform(c) => *c,
            PrimColor::PerElement(b) => b.data.get(i).map_or(Color::TRANSPARENT, |v| unpremul_u32(*v)),
            PrimColor::Values(b, _) => match (b.data.get(i), &self.mapper) {
                (Some(v), Some(m)) => m.color(*v),
                _ => Color::TRANSPARENT,
            },
        }
    }

    /// The color halfway between elements `i` and `j` (values are averaged before mapping).
    fn mid(&self, i: usize, j: usize) -> Color {
        match self.color {
            PrimColor::Values(b, _) => match (b.data.get(i), b.data.get(j), &self.mapper) {
                (Some(x), Some(y), Some(m)) => m.color(0.5 * (x + y)),
                _ => Color::TRANSPARENT,
            },
            _ => self.at(i).lerp(self.at(j), 0.5),
        }
    }
}

/// Makie's FastShading of one vertex (CairoMakie's `_calculate_shaded_vertexcolors`).
fn shade(v: &View3d, m: &crate::scene::drawlist::Material, p: [f32; 3], normal: [f32; 3], c: Color) -> Color {
    let n: [f64; 3] = std::array::from_fn(|i| normal[i] as f64 * v.normal_scale[i]);
    let ln = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if !(ln > 0.0) {
        return c;
    }
    let nn = n.map(|x| -x / ln); // -N
    let w: [f64; 3] = std::array::from_fn(|i| p[i] as f64 * v.world_scale[i] + v.world_offset[i] - v.eye[i]);
    let lw = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt().max(1e-300);
    let l = v.light_dir;
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let diff = dot(l, nn).max(0.0);
    let h: [f64; 3] = std::array::from_fn(|i| l[i] + w[i] / lw);
    let lh = dot(h, h).sqrt().max(1e-300);
    let spec = (dot(h, nn) / lh).max(0.0).powf(m.shininess as f64);
    let (amb, lc) = (v.ambient as f64, v.light_color as f64);
    let k = amb + lc * diff * m.diffuse as f64;
    let s = lc * m.specular as f64 * spec;
    let ch = |x: f32| (k * x as f64 + s).clamp(0.0, 1.0) as f32;
    Color::rgba(ch(c.r), ch(c.g), ch(c.b), c.a)
}

/// Projects and sorts a run of 3D items of one depth group; returns 2D figure-space primitives
/// in painter's order.
pub(super) fn flatten(items: &[Item]) -> Vec<Prim> {
    let mut els: Vec<(f64, El)> = Vec::new();
    for item in items {
        match &item.prim {
            Prim::Mesh3d(m) => {
                let v = &m.view;
                let cs = Colors::new(&m.color);
                for (t, tri) in m.verts.data.as_chunks::<3>().0.iter().enumerate() {
                    if !tri.iter().all(|q| in_box(v, q.pos)) {
                        continue;
                    }
                    let (Some(a), Some(b), Some(c)) =
                        (project(v, tri[0].pos), project(v, tri[1].pos), project(v, tri[2].pos))
                    else {
                        continue;
                    };
                    let col: [Color; 3] = std::array::from_fn(|k| {
                        let c0 = cs.at(3 * t + k);
                        match &m.shading {
                            Some(mat) => shade(v, mat, tri[k].pos, tri[k].normal, c0),
                            None => c0,
                        }
                    });
                    els.push(((a.1 + b.1 + c.1) / 3.0, El::Tri([a.0, b.0, c.0], col)));
                }
            }
            Prim::Lines3d(l) => {
                let v = &l.view;
                let cs = Colors::new(&l.color);
                let p = &l.pts.data;
                for i in 0..p.len().saturating_sub(1) {
                    let (a, b) = (p[i], p[i + 1]);
                    if !(a.iter().chain(&b).all(|c| c.is_finite())) {
                        continue;
                    }
                    let Some((t0, t1)) = clip_segment(v, a, b) else { continue };
                    let at = |t: f32| std::array::from_fn::<f32, 3, _>(|k| a[k] + t * (b[k] - a[k]));
                    let (Some(pa), Some(pb)) = (project(v, at(t0)), project(v, at(t1))) else { continue };
                    let c = cs.mid(i, i + 1);
                    els.push((0.5 * (pa.1 + pb.1), El::Seg([pa.0, pb.0], c, l.width)));
                }
            }
            Prim::Markers3d(m) => {
                let v = &m.view;
                let cs = Colors::new(&m.color);
                for (i, p) in m.pos.data.iter().enumerate() {
                    if !p.iter().all(|c| c.is_finite()) || !in_box(v, *p) {
                        continue;
                    }
                    let Some((q, z)) = project(v, *p) else { continue };
                    let size = m.sizes.as_ref().map_or(Some(m.size), |s| s.data.get(i).copied());
                    let Some(size) = size else { continue };
                    let style = MarkStyle {
                        marker: m.marker,
                        size,
                        stroke_color: m.stroke_color,
                        stroke_width: m.stroke_width,
                    };
                    els.push((z, El::Mark(q, cs.at(i), style)));
                }
            }
            _ => {}
        }
    }
    // Back to front (larger NDC depth is farther); equal depths keep their order.
    els.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut out = Vec::new();
    let mut tris: Vec<MeshVertex> = Vec::new();
    let mut segs: Option<(Color, f32, Vec<[f32; 2]>)> = None;
    let mut marks: Option<(MarkStyle, Vec<[f32; 2]>, Vec<u32>)> = None;
    let f32p = |p: [f64; 2]| [p[0] as f32, p[1] as f32];
    fn flush_tris(out: &mut Vec<Prim>, tris: &mut Vec<MeshVertex>) {
        if !tris.is_empty() {
            out.push(Prim::Mesh(MeshPrim { verts: Buf::transient(std::mem::take(tris)) }));
        }
    }
    fn flush_segs(out: &mut Vec<Prim>, segs: &mut Option<(Color, f32, Vec<[f32; 2]>)>) {
        if let Some((c, w, pts)) = segs.take() {
            out.push(Prim::Lines(LinesPrim {
                pts: Buf::transient(pts),
                color: PrimColor::Uniform(c),
                width: w,
                pattern: None,
                cap: LineCap::Round,
                join: JoinStyle::Round,
                miter_limit: std::f32::consts::FRAC_PI_3,
                segments: true,
                closed: false,
                append: false,
            }));
        }
    }
    fn flush_marks(out: &mut Vec<Prim>, marks: &mut Option<(MarkStyle, Vec<[f32; 2]>, Vec<u32>)>) {
        if let Some((s, pos, cols)) = marks.take() {
            let first = cols[0];
            let color = if cols.iter().all(|c| *c == first) {
                PrimColor::Uniform(unpremul_u32(first))
            } else {
                PrimColor::PerElement(Buf::transient(cols))
            };
            out.push(Prim::Markers(MarkersPrim {
                pos: Buf::transient(pos),
                color,
                size: s.size,
                sizes: None,
                marker: s.marker,
                stroke_color: s.stroke_color,
                stroke_width: s.stroke_width,
                rotation: 0.0,
            }));
        }
    }
    for (_, el) in els {
        match el {
            El::Tri(p, c) => {
                flush_segs(&mut out, &mut segs);
                flush_marks(&mut out, &mut marks);
                // Grown by a fraction of a unit (edges offset outward, miters capped), so
                // antialiased edges of neighbouring triangles leave no hairline seams (nearer
                // triangles cover the overlaps).
                let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
                let sgn = if area < 0.0 { -1.0 } else { 1.0 };
                let normal = |a: [f64; 2], b: [f64; 2]| {
                    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                    let l = dx.hypot(dy);
                    if l > 0.0 { [sgn * dy / l, -sgn * dx / l] } else { [0.0, 0.0] }
                };
                for k in 0..3 {
                    let (prev, next) = (p[(k + 2) % 3], p[(k + 1) % 3]);
                    let (na, nb) = (normal(prev, p[k]), normal(p[k], next));
                    let m = [na[0] + nb[0], na[1] + nb[1]];
                    let den = 1.0 + na[0] * nb[0] + na[1] * nb[1];
                    let mut o = if den > 1e-6 { [SEAM * m[0] / den, SEAM * m[1] / den] } else { [0.0, 0.0] };
                    let ol = o[0].hypot(o[1]);
                    if ol > 3.0 * SEAM {
                        o = [o[0] * 3.0 * SEAM / ol, o[1] * 3.0 * SEAM / ol];
                    }
                    let q = [p[k][0] + o[0], p[k][1] + o[1]];
                    tris.push(MeshVertex { pos: f32p(q), color: c[k].to_premul_u32() });
                }
            }
            El::Seg(p, c, w) => {
                flush_tris(&mut out, &mut tris);
                flush_marks(&mut out, &mut marks);
                match &mut segs {
                    Some((sc, sw, pts)) if *sc == c && *sw == w => pts.extend([f32p(p[0]), f32p(p[1])]),
                    _ => {
                        flush_segs(&mut out, &mut segs);
                        segs = Some((c, w, vec![f32p(p[0]), f32p(p[1])]));
                    }
                }
            }
            El::Mark(p, c, s) => {
                flush_tris(&mut out, &mut tris);
                flush_segs(&mut out, &mut segs);
                match &mut marks {
                    Some((ms, pos, cols)) if *ms == s => {
                        pos.push(f32p(p));
                        cols.push(c.to_premul_u32());
                    }
                    _ => {
                        flush_marks(&mut out, &mut marks);
                        marks = Some((s, vec![f32p(p)], vec![c.to_premul_u32()]));
                    }
                }
            }
        }
    }
    flush_tris(&mut out, &mut tris);
    flush_segs(&mut out, &mut segs);
    flush_marks(&mut out, &mut marks);
    out
}
