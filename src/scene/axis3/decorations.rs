//! Axis3 decorations, ported from Makie's `axis3d.jl` (`add_panel!`,
//! `add_gridlines_and_frames!`, `add_ticks_and_ticklabels!`, the title): panels, grid lines and
//! frame lines on the three far sides of the box, ticks and tick labels on the edges facing the
//! viewer, axis labels offset from those edges, and the title above the layout box.
//!
//! Geometry is computed in Makie's figure pixels (y up) and flipped when emitted.

use super::Axis3Frame;
use super::camera::mod1;
use crate::color::Color;
use crate::scene::axis::z;
use crate::scene::drawlist::{
    Buf, Emitter, GlyphsPrim, LinesPrim, MeshPrim, MeshVertex, Prim, PrimColor, RectPrim, Space,
};
use crate::style::{JoinStyle, LineCap};
use crate::text::{Font, RichText};
use std::f64::consts::PI;

/// The attributes of one dimension.
struct Dim {
    ticksvisible: bool,
    ticksize: f64,
    tickwidth: f64,
    tickcolor: Color,
    ticklabelsvisible: bool,
    ticklabelsize: f64,
    ticklabelcolor: Color,
    ticklabelfont: Font,
    ticklabelpad: f64,
    gridvisible: bool,
    gridcolor: Color,
    gridwidth: f64,
    spinesvisible: bool,
    spinecolor_1: Color,
    spinecolor_2: Color,
    spinecolor_3: Color,
    spinecolor_4: Color,
    spinewidth: f64,
    label: RichText,
    labelsize: f64,
    labelcolor: Color,
    labelfont: Font,
    labelvisible: bool,
    labelrotation: Option<f64>,
    labeloffset: f64,
}

macro_rules! dims {
    ($r:expr, $d:expr; $($f:ident: $x:ident $y:ident $z:ident),* $(,)?) => {
        match $d {
            0 => Dim { $($f: $r.$x.clone()),* },
            1 => Dim { $($f: $r.$y.clone()),* },
            _ => Dim { $($f: $r.$z.clone()),* },
        }
    };
}

fn dim(f: &Axis3Frame, d: usize) -> Dim {
    let r = &f.attrs;
    dims!(r, d;
        ticksvisible: xticksvisible yticksvisible zticksvisible,
        ticksize: xticksize yticksize zticksize,
        tickwidth: xtickwidth ytickwidth ztickwidth,
        tickcolor: xtickcolor ytickcolor ztickcolor,
        ticklabelsvisible: xticklabelsvisible yticklabelsvisible zticklabelsvisible,
        ticklabelsize: xticklabelsize yticklabelsize zticklabelsize,
        ticklabelcolor: xticklabelcolor yticklabelcolor zticklabelcolor,
        ticklabelfont: xticklabelfont yticklabelfont zticklabelfont,
        ticklabelpad: xticklabelpad yticklabelpad zticklabelpad,
        gridvisible: xgridvisible ygridvisible zgridvisible,
        gridcolor: xgridcolor ygridcolor zgridcolor,
        gridwidth: xgridwidth ygridwidth zgridwidth,
        spinesvisible: xspinesvisible yspinesvisible zspinesvisible,
        spinecolor_1: xspinecolor_1 yspinecolor_1 zspinecolor_1,
        spinecolor_2: xspinecolor_2 yspinecolor_2 zspinecolor_2,
        spinecolor_3: xspinecolor_3 yspinecolor_3 zspinecolor_3,
        spinecolor_4: xspinecolor_4 yspinecolor_4 zspinecolor_4,
        spinewidth: xspinewidth yspinewidth zspinewidth,
        label: xlabel ylabel zlabel,
        labelsize: xlabelsize ylabelsize zlabelsize,
        labelcolor: xlabelcolor ylabelcolor zlabelcolor,
        labelfont: xlabelfont ylabelfont zlabelfont,
        labelvisible: xlabelvisible ylabelvisible zlabelvisible,
        labelrotation: xlabelrotation ylabelrotation zlabelrotation,
        labeloffset: xlabeloffset ylabeloffset zlabeloffset,
    )
}

/// Makie's `mi1`, `mi2`, `mi3`: whether the far side of the box is the minimum along x, y, z.
fn mis(f: &Axis3Frame) -> [bool; 3] {
    let az = mod1(f.attrs.azimuth, 2.0 * PI);
    [!(PI / 2.0..1.5 * PI).contains(&az), (0.0..PI).contains(&az), f.attrs.elevation > 0.0]
}

/// `(miv, min1, min2)` of dimension `d` (Makie's argument order per dimension).
fn flags(f: &Axis3Frame, d: usize) -> (bool, bool, bool) {
    let [m1, m2, m3] = mis(f);
    match d {
        0 => (m1, m2, m3),
        1 => (m2, m1, m3),
        _ => (m3, m1, m2),
    }
}

/// Makie's `dim1` / `dim2` (0-based).
fn d1(d: usize) -> usize {
    if d == 0 { 1 } else { 0 }
}
fn d2(d: usize) -> usize {
    if d == 2 { 1 } else { 2 }
}

/// Makie's `dimpoint(dim, v, v1, v2)`.
fn dpoint(d: usize, v: f64, v1: f64, v2: f64) -> [f64; 3] {
    let mut p = [0.0; 3];
    p[d] = v;
    p[d1(d)] = v1;
    p[d2(d)] = v2;
    p
}

fn reversed(f: &Axis3Frame) -> [bool; 3] {
    [f.attrs.xreversed, f.attrs.yreversed, f.attrs.zreversed]
}

fn lo(f: &Axis3Frame, i: usize) -> f64 {
    f.limits[2 * i]
}
fn hi(f: &Axis3Frame, i: usize) -> f64 {
    f.limits[2 * i + 1]
}

/// Julia's `x ≈ y` (default tolerances).
fn approx(x: f64, y: f64) -> bool {
    x == y || (x - y).abs() <= f64::EPSILON.sqrt() * x.abs().max(y.abs())
}

/// Grid line endpoints (data space) of dimension `d`: `(gridline1, gridline2)`, pairs of points.
pub(super) fn gridlines(f: &Axis3Frame, d: usize) -> (Vec<[f64; 3]>, Vec<[f64; 3]>) {
    let (_, min1, min2) = flags(f, d);
    let rev = reversed(f);
    let (a, b) = (d1(d), d2(d));
    let f1 = if min1 ^ rev[a] { lo(f, a) } else { hi(f, a) };
    let f2 = if min2 ^ rev[b] { lo(f, b) } else { hi(f, b) };
    let ticks = f.ticks[d].values.iter().filter(|t| !approx(**t, lo(f, d)) && !approx(**t, hi(f, d)));
    let (mut g1, mut g2) = (Vec::new(), Vec::new());
    for &t in ticks {
        g1.extend([dpoint(d, t, f1, lo(f, b)), dpoint(d, t, f1, hi(f, b))]);
        g2.extend([dpoint(d, t, lo(f, a), f2), dpoint(d, t, hi(f, a), f2)]);
    }
    (g1, g2)
}

/// Frame line endpoints of dimension `d`: the three back edges (pairs), and the front edge.
pub(super) fn framelines(f: &Axis3Frame, d: usize) -> (Vec<[f64; 3]>, [[f64; 3]; 2]) {
    let (_, min1, min2) = flags(f, d);
    let rev = reversed(f);
    let (a, b) = (d1(d), d2(d));
    let m1 = min1 ^ rev[a];
    let m2 = min2 ^ rev[b];
    let pick = |mi: bool, i: usize| if mi { lo(f, i) } else { hi(f, i) };
    let (dl, dh) = (lo(f, d), hi(f, d));
    let back = vec![
        dpoint(d, dl, pick(!m1, a), pick(m2, b)),
        dpoint(d, dh, pick(!m1, a), pick(m2, b)),
        dpoint(d, dl, pick(m1, a), pick(m2, b)),
        dpoint(d, dh, pick(m1, a), pick(m2, b)),
        dpoint(d, dl, pick(m1, a), pick(!m2, b)),
        dpoint(d, dh, pick(m1, a), pick(!m2, b)),
    ];
    let front = [dpoint(d, dl, pick(!m1, a), pick(!m2, b)), dpoint(d, dh, pick(!m1, a), pick(!m2, b))];
    (back, front)
}

/// Ticks of one dimension in Makie pixels (y up).
pub(super) struct TickGeom {
    pub segments: Vec<[[f64; 2]; 2]>,
    pub label_pos: Vec<[f64; 2]>,
    /// `(h, v)`: 0 = left / bottom, 0.5 = center, 1 = right / top.
    pub align: (f64, f64),
}

fn normalize2(v: [f64; 2]) -> [f64; 2] {
    let n = v[0].hypot(v[1]);
    if n > 0.0 { [v[0] / n, v[1] / n] } else { [0.0, 0.0] }
}

/// Makie's tick segments, tick label positions and alignment of dimension `d`.
pub(super) fn ticks(f: &Axis3Frame, d: usize) -> TickGeom {
    let (miv, min1, min2) = flags(f, d);
    let rev = reversed(f);
    let (a, b) = (d1(d), d2(d));
    let at = dim(f, d);
    let f1 = if !(min1 ^ rev[a]) { lo(f, a) } else { hi(f, a) };
    let f2 = if min2 ^ rev[b] { lo(f, b) } else { hi(f, b) };
    let f1_oppo = if min1 ^ rev[a] { lo(f, a) } else { hi(f, a) };
    let f2_oppo = if !(min2 ^ rev[b]) { lo(f, b) } else { hi(f, b) };
    let (df1, df2) = (f1 - f1_oppo, f2 - f2_oppo);
    let az_deg = mod1(f.attrs.azimuth.to_degrees(), 180.0);
    let mut segments = Vec::new();
    let mut label_pos = Vec::new();
    for &t in &f.ticks[d].values {
        let p1 = dpoint(d, t, f1, f2);
        let p2 = if d == 2 && !(45.0..=135.0).contains(&az_deg) {
            dpoint(d, t, f1, f2 + df2)
        } else {
            dpoint(d, t, f1 + df1, f2)
        };
        let (Some(pp1), Some(pp2)) = (f.project_up_any(p1), f.project_up_any(p2)) else { continue };
        let dir = normalize2([pp2[0] - pp1[0], pp2[1] - pp1[1]]);
        let end = [pp1[0] + at.ticksize * dir[0], pp1[1] + at.ticksize * dir[1]];
        segments.push([pp1, end]);
        let off = normalize2([end[0] - pp1[0], end[1] - pp1[1]]);
        label_pos.push([end[0] + at.ticklabelpad * off[0], end[1] + at.ticklabelpad * off[1]]);
    }
    let lr = |right: bool| if right { 1.0 } else { 0.0 };
    let tb = |top: bool| if top { 1.0 } else { 0.0 };
    let align = match d {
        0 => (lr(miv ^ min1), tb(min2)),
        1 => (lr(!(miv ^ min1)), tb(min2)),
        _ => (lr(!(min1 ^ min2)), 0.5),
    };
    TickGeom { segments, label_pos, align }
}

/// An axis label's placement in Makie pixels (y up).
pub(super) struct LabelGeom {
    pub pos: [f64; 2],
    pub rotation: f64,
    pub align: (f64, f64),
}

/// Makie's axis label position, rotation and alignment of dimension `d`.
pub(super) fn label(f: &Axis3Frame, d: usize) -> LabelGeom {
    let (_, min1, min2) = flags(f, d);
    let rev = reversed(f);
    let (a, b) = (d1(d), d2(d));
    let at = dim(f, d);
    let minr1 = min1 ^ rev[a];
    let minr2 = min2 ^ rev[b];
    let f1 = if !minr1 { lo(f, a) } else { hi(f, a) };
    let f2 = if minr2 { lo(f, b) } else { hi(f, b) };
    let (Some(pp1), Some(pp2)) =
        (f.project_up_any(dpoint(d, lo(f, d), f1, f2)), f.project_up_any(dpoint(d, hi(f, d), f1, f2)))
    else {
        return LabelGeom { pos: [f64::NAN; 2], rotation: 0.0, align: (0.5, 1.0) };
    };
    let mid = [0.5 * (pp1[0] + pp2[0]), 0.5 * (pp1[1] + pp2[1])];
    let diff = [pp2[0] - pp1[0], pp2[1] - pp1[1]];
    let s = min1 ^ min2 ^ rev[d];
    let sign = if (d == 1) == s { 1.0 } else { -1.0 };
    let n = normalize2([sign * diff[0], sign * diff[1]]);
    // Rotated by +90° (counter-clockwise, y up).
    let ov = [-n[1], n[0]];
    let pos = [mid[0] + at.labeloffset * ov[0], mid[1] + at.labeloffset * ov[1]];
    let ang = ov[1].atan2(ov[0]);
    // Julia's `%` keeps the sign of the dividend (like Rust's).
    let mut up = ((ang + PI / 2.0 + PI / 2.0) % PI) - PI / 2.0;
    let flip = up < -(88f64.to_radians());
    if flip {
        up += PI;
    }
    let rotation = at.labelrotation.unwrap_or(up);
    let valign = if ov[1] > 0.0 || flip { 0.0 } else { 1.0 };
    LabelGeom { pos, rotation, align: (0.5, valign) }
}

/// The title's anchor in Makie pixels (y up); aligned `(titlealign, bottom)`.
pub(super) fn title_anchor(f: &Axis3Frame) -> [f64; 2] {
    let b = f.bbox;
    let top_up = f.fig_h - b.y;
    [b.x + f.attrs.titlealign.frac() * b.w, top_up + f.attrs.titlegap]
}

fn segments_prim(pts: Vec<[f32; 2]>, colors: Option<Vec<u32>>, color: Color, width: f64) -> Prim {
    Prim::Lines(LinesPrim {
        pts: Buf::transient(pts),
        color: match colors {
            Some(c) => PrimColor::PerElement(Buf::transient(c)),
            None => PrimColor::Uniform(color),
        },
        width: width as f32,
        pattern: None,
        cap: LineCap::Butt,
        join: JoinStyle::Miter,
        miter_limit: std::f32::consts::FRAC_PI_3,
        segments: true,
        closed: false,
        append: false,
    })
}

/// Projects data-space segment endpoints to figure units (y down); drops pairs behind the camera.
fn project_pairs(f: &Axis3Frame, pts: &[[f64; 3]]) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(pts.len());
    for pair in pts.as_chunks::<2>().0 {
        if let (Some(a), Some(b)) = (f.project_up(pair[0]), f.project_up(pair[1])) {
            let (a, b) = (f.down(a), f.down(b));
            out.extend([[a[0] as f32, a[1] as f32], [b[0] as f32, b[1] as f32]]);
        }
    }
    out
}

/// Background, panels, grid lines and back frame lines (below the plots).
pub(super) fn emit_back(em: &mut Emitter, f: &Axis3Frame) {
    let r = &f.attrs;
    if r.backgroundcolor.a > 0.0 {
        let rect = RectPrim { rect: f.area, color: r.backgroundcolor, snap: false };
        em.push(z::BACKGROUND, None, Space::Figure, Prim::Rects(vec![rect]));
    }
    // Panels: (dims of the plane, normal dim, which far-side flag, color, visible).
    let [m1, m2, m3] = mis(f);
    let panels = [
        (0, 1, 2, m3, r.xypanelcolor, r.xypanelvisible),
        (1, 2, 0, m1, r.yzpanelcolor, r.yzpanelvisible),
        (0, 2, 1, m2, r.xzpanelcolor, r.xzpanelvisible),
    ];
    let mut verts = Vec::new();
    for (i, j, k, mi, color, visible) in panels {
        if !visible || color.a <= 0.0 {
            continue;
        }
        let off = if mi { lo(f, k) } else { hi(f, k) };
        let corner = |u: f64, v: f64| {
            let mut p = [0.0; 3];
            p[i] = u;
            p[j] = v;
            p[k] = off;
            f.project_up(p).map(|q| f.down(q))
        };
        let c = [
            corner(lo(f, i), lo(f, j)),
            corner(hi(f, i), lo(f, j)),
            corner(hi(f, i), hi(f, j)),
            corner(lo(f, i), hi(f, j)),
        ];
        let [Some(a), Some(b), Some(cc), Some(dd)] = c else { continue };
        let col = color.to_premul_u32();
        for p in [a, b, cc, a, cc, dd] {
            verts.push(MeshVertex { pos: [p[0] as f32, p[1] as f32], color: col });
        }
    }
    if !verts.is_empty() {
        em.push(z::BACKGROUND, None, Space::Figure, Prim::Mesh(MeshPrim { verts: Buf::transient(verts) }));
    }
    for d in 0..3 {
        let at = dim(f, d);
        if !at.gridvisible {
            continue;
        }
        let (g1, g2) = gridlines(f, d);
        let pts: Vec<[f64; 3]> = g1.into_iter().chain(g2).collect();
        let proj = project_pairs(f, &pts);
        if !proj.is_empty() {
            em.push(z::GRID, None, Space::Figure, segments_prim(proj, None, at.gridcolor, at.gridwidth));
        }
    }
    for d in 0..3 {
        let at = dim(f, d);
        if !at.spinesvisible {
            continue;
        }
        let (back, _) = framelines(f, d);
        let proj = project_pairs(f, &back);
        if proj.len() == 6 {
            let cs = [at.spinecolor_1, at.spinecolor_2, at.spinecolor_3];
            let colors = cs.iter().flat_map(|c| [c.to_premul_u32(); 2]).collect();
            em.push(z::GRID, None, Space::Figure, segments_prim(proj, Some(colors), Color::TRANSPARENT, at.spinewidth));
        }
    }
}

/// Front spines, ticks, tick labels, axis labels and the title (above the plots).
pub(super) fn emit_front(em: &mut Emitter, f: &Axis3Frame) {
    let r = &f.attrs;
    let mut glyphs = Vec::new();
    for d in 0..3 {
        let at = dim(f, d);
        if r.front_spines && at.spinesvisible {
            let (_, front) = framelines(f, d);
            let proj = project_pairs(f, &front);
            if !proj.is_empty() {
                em.push(z::SPINES, None, Space::Figure, segments_prim(proj, None, at.spinecolor_4, at.spinewidth));
            }
        }
        let t = ticks(f, d);
        if at.ticksvisible && !t.segments.is_empty() {
            let pts =
                t.segments.iter().flat_map(|s| s.map(|p| f.down(p))).map(|p| [p[0] as f32, p[1] as f32]).collect();
            em.push(z::TICKS, None, Space::Figure, segments_prim(pts, None, at.tickcolor, at.tickwidth));
        }
        if at.ticklabelsvisible {
            for (label, pos) in f.ticks[d].labels.iter().zip(&t.label_pos) {
                let l = crate::text::layout(label, at.ticklabelsize, at.ticklabelfont, at.ticklabelcolor);
                glyphs.extend(crate::text::place(&l, f.down(*pos), t.align, 0.0));
            }
        }
        if at.labelvisible {
            let lg = label(f, d);
            if lg.pos.iter().all(|v| v.is_finite()) {
                let l = crate::text::layout(&at.label, at.labelsize, at.labelfont, at.labelcolor);
                glyphs.extend(crate::text::place(&l, f.down(lg.pos), lg.align, lg.rotation));
            }
        }
    }
    if r.titlevisible && !r.title.spans.iter().all(|s| s.text.is_empty()) {
        let l = crate::text::layout(&r.title, r.titlesize, r.titlefont, r.titlecolor);
        glyphs.extend(crate::text::place(&l, f.down(title_anchor(f)), (r.titlealign.frac(), 0.0), 0.0));
    }
    if !glyphs.is_empty() {
        em.push(z::TEXT, None, Space::Figure, Prim::Glyphs(GlyphsPrim { glyphs }));
    }
}
