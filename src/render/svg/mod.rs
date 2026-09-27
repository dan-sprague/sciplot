//! SVG backend: a deterministic, CPU-only writer for a `DrawList`.
//!
//! The document is `W x H` figure units in its `viewBox`, sized `W * pt_per_unit` points. Items are
//! written in painter's order; data-space items are mapped to figure units on the CPU in f64
//! through `AxisXform::affine(1.0)`, so stroke widths and marker sizes are never distorted.
//! Numbers have at most 3 decimals and ids are sequential, so output is byte-stable.

pub(crate) mod field;
mod marker;
mod num;
#[cfg(test)]
mod tests;
mod three_d;

use crate::color::Color;
use crate::scene::drawlist::{
    DrawList, FieldPrim, GlyphsPrim, Item, LinesPrim, MarkersPrim, MeshPrim, Prim, PrimColor, Rect, RectPrim, Space,
};
use crate::style::{JoinStyle, LineCap, Marker};
use crate::text::Font;
use crate::text::outline::{PathSeg, glyph_path, units_per_em};
use base64::prelude::*;
use field::regular_image;
use num::{N, paint, unpremul_u32};
use std::collections::HashMap;
use std::fmt::Write as _;

/// Marker counts above this make a warning (vector scatters get huge).
const MANY_MARKERS: usize = 100_000;

/// Options for [`document`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct SvgOptions {
    /// Points per figure unit for the root `width`/`height`.
    pub pt_per_unit: f64,
    /// Snap decoration rectangles and clip rectangles to this device-pixel grid like the GPU
    /// backend does (used when rasterizing on the CPU; `None` for vector output).
    pub snap_ppu: Option<f64>,
}

impl Default for SvgOptions {
    fn default() -> Self {
        SvgOptions { pt_per_unit: 0.75, snap_ppu: None }
    }
}

/// Writes `dl` as a standalone SVG document.
pub(crate) fn document(dl: &DrawList, opts: &SvgOptions) -> String {
    let mut w = Writer {
        dl,
        opts: *opts,
        defs: String::new(),
        body: String::new(),
        ids: 0,
        clips: Vec::new(),
        open_clip: None,
        markers: HashMap::new(),
        glyphs: HashMap::new(),
    };
    let n_markers: usize = dl
        .items
        .iter()
        .map(|i| match &i.prim {
            Prim::Markers(m) => m.pos.len(),
            _ => 0,
        })
        .sum();
    if n_markers > MANY_MARKERS {
        crate::warn_once("an SVG with more than 100k markers is large and slow to view; consider PNG output");
    }
    let mut i = 0;
    while i < dl.items.len() {
        let item = &dl.items[i];
        match three_d::group(&item.prim) {
            // A run of 3D items of one Axis3: projected and depth-sorted together.
            Some(g) => {
                let n = dl.items[i..].iter().take_while(|it| three_d::group(&it.prim) == Some(g)).count();
                w.items3d(&dl.items[i..i + n]);
                i += n;
            }
            None => {
                w.item(item);
                i += 1;
            }
        }
    }
    w.finish()
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MarkerKey {
    marker: Marker,
    size: i64,
    rotation: i64,
    stroke: [u8; 4],
    stroke_width: i64,
    outer_only: bool,
}

struct Writer<'a> {
    dl: &'a DrawList,
    opts: SvgOptions,
    defs: String,
    body: String,
    ids: u32,
    clips: Vec<(Rect, u32)>,
    open_clip: Option<u32>,
    markers: HashMap<MarkerKey, u32>,
    glyphs: HashMap<(Font, u16), Option<u32>>,
}

/// Maps a local point through `[sx, sy, tx, ty]`; `None` for non-finite points.
fn map(xf: [f64; 4], p: [f32; 2]) -> Option<[f64; 2]> {
    let q = [p[0] as f64 * xf[0] + xf[2], p[1] as f64 * xf[1] + xf[3]];
    (q[0].is_finite() && q[1].is_finite()).then_some(q)
}

/// Snaps the center of a thin line to whole device pixels (as `render::gpu` does for rects).
fn snap_center(c: f64, width_px: f64) -> f64 {
    if (width_px.round() as i64) % 2 == 1 { c.floor() + 0.5 } else { c.round() }
}

fn rect_attrs(out: &mut String, x: f64, y: f64, w: f64, h: f64) {
    let _ = write!(out, " x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"", N(x), N(y), N(w), N(h));
}

/// Writes `items` of one color as a single element when possible, else a `<g fill>` group.
fn fill_group(out: &mut String, color: Color, children: &[String]) {
    match children {
        [] => {}
        [one] => {
            // `<rect .../>` or `<path .../>`: add the fill before the closing `/>`.
            let head = one.strip_suffix("/>").unwrap_or(one);
            out.push_str(head);
            paint(out, "fill", color);
            out.push_str("/>\n");
        }
        _ => {
            out.push_str("<g");
            paint(out, "fill", color);
            out.push_str(">\n");
            for c in children {
                out.push_str(c);
                out.push('\n');
            }
            out.push_str("</g>\n");
        }
    }
}

impl Writer<'_> {
    fn next_id(&mut self) -> u32 {
        self.ids += 1;
        self.ids
    }

    fn xform(&self, space: Space) -> [f64; 4] {
        match space {
            Space::Figure => [1.0, 1.0, 0.0, 0.0],
            Space::Data(i) => self.dl.axes.get(i as usize).map_or([1.0, 1.0, 0.0, 0.0], |a| a.affine(1.0)),
        }
    }

    fn item(&mut self, item: &Item) {
        self.set_clip(item.clip);
        let xf = self.xform(item.space);
        match &item.prim {
            Prim::Rects(r) => self.rects(r, xf),
            Prim::Mesh(m) => self.mesh(m, xf),
            Prim::Markers(m) => self.markers(m, xf, item.clip),
            Prim::Lines(l) => self.lines(l, xf),
            Prim::Glyphs(g) => self.glyphs(g, xf),
            Prim::Field(f) => self.field(f, xf),
            Prim::Lines3d(_) | Prim::Markers3d(_) | Prim::Mesh3d(_) => self.items3d(std::slice::from_ref(item)),
        }
    }

    /// 3D items of one depth group, in painter's order (see `three_d`).
    fn items3d(&mut self, items: &[Item]) {
        let Some(first) = items.first() else { return };
        let clip = first.clip;
        self.set_clip(clip);
        let xf = [1.0, 1.0, 0.0, 0.0];
        for prim in three_d::flatten(items) {
            match &prim {
                Prim::Mesh(m) => self.mesh(m, xf),
                Prim::Lines(l) => self.lines(l, xf),
                Prim::Markers(m) => self.markers(m, xf, clip),
                _ => {}
            }
        }
    }

    fn set_clip(&mut self, clip: Option<Rect>) {
        let want = clip.map(|r| self.clip_id(r));
        if want == self.open_clip {
            return;
        }
        if self.open_clip.is_some() {
            self.body.push_str("</g>\n");
        }
        if let Some(id) = want {
            let _ = writeln!(self.body, "<g clip-path=\"url(#c{id})\">");
        }
        self.open_clip = want;
    }

    fn clip_id(&mut self, r: Rect) -> u32 {
        if let Some((_, id)) = self.clips.iter().find(|(c, _)| *c == r) {
            return *id;
        }
        let id = self.next_id();
        self.clips.push((r, id));
        let (mut x0, mut y0, mut x1, mut y1) = (r.x, r.y, r.right(), r.bottom());
        if let Some(ppu) = self.opts.snap_ppu {
            // The GPU scissor is whole device pixels.
            [x0, y0, x1, y1] = [x0, y0, x1, y1].map(|v| (v * ppu).round() / ppu);
        }
        let _ = write!(self.defs, "<clipPath id=\"c{id}\"><rect");
        rect_attrs(&mut self.defs, x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0));
        self.defs.push_str("/></clipPath>\n");
        id
    }

    /// Decoration rectangles (always figure space).
    fn rects(&mut self, rs: &[RectPrim], xf: [f64; 4]) {
        let mut runs: Vec<(Color, Vec<String>)> = Vec::new();
        for r in rs {
            let (x0, y0) = (r.rect.x * xf[0] + xf[2], r.rect.y * xf[1] + xf[3]);
            let (x1, y1) = (r.rect.right() * xf[0] + xf[2], r.rect.bottom() * xf[1] + xf[3]);
            let mut rect = Rect::new(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
            if r.color.a <= 0.0 || !(rect.w > 0.0 && rect.h > 0.0 && rect.x.is_finite() && rect.y.is_finite()) {
                continue;
            }
            if let (Some(ppu), true) = (self.opts.snap_ppu, r.snap) {
                let (w, h) = (rect.w * ppu, rect.h * ppu);
                if w <= h {
                    rect.x = (snap_center(rect.x * ppu + 0.5 * w, w) - 0.5 * w) / ppu;
                }
                if h <= w {
                    rect.y = (snap_center(rect.y * ppu + 0.5 * h, h) - 0.5 * h) / ppu;
                }
            }
            let mut s = String::from("<rect");
            rect_attrs(&mut s, rect.x, rect.y, rect.w, rect.h);
            s.push_str("/>");
            match runs.last_mut() {
                Some((c, v)) if *c == r.color => v.push(s),
                _ => runs.push((r.color, vec![s])),
            }
        }
        for (c, v) in runs {
            fill_group(&mut self.body, c, &v);
        }
    }

    /// Triangles: consecutive same-colored triangles become one path (no seams between them),
    /// each triangle wound the same way so the nonzero rule gives their union.
    fn mesh(&mut self, m: &MeshPrim, xf: [f64; 4]) {
        let mut runs: Vec<(Color, String)> = Vec::new();
        for tri in m.verts.data.as_chunks::<3>().0 {
            let color = if tri[0].color == tri[1].color && tri[1].color == tri[2].color {
                unpremul_u32(tri[0].color)
            } else {
                // Per-vertex gradients are not representable per triangle: use the mean.
                let cs = tri.iter().map(|v| unpremul_u32(v.color));
                let (mut r, mut g, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);
                for c in cs {
                    r += c.r / 3.0;
                    g += c.g / 3.0;
                    b += c.b / 3.0;
                    a += c.a / 3.0;
                }
                Color::rgba(r, g, b, a)
            };
            if color.a <= 0.0 {
                continue;
            }
            let (Some(a), Some(mut b), Some(mut c)) = (map(xf, tri[0].pos), map(xf, tri[1].pos), map(xf, tri[2].pos))
            else {
                continue;
            };
            if (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) < 0.0 {
                std::mem::swap(&mut b, &mut c);
            }
            let tri = format!("M{} {}L{} {}L{} {}Z", N(a[0]), N(a[1]), N(b[0]), N(b[1]), N(c[0]), N(c[1]));
            match runs.last_mut() {
                Some((rc, d)) if *rc == color => d.push_str(&tri),
                _ => runs.push((color, tri)),
            }
        }
        for (c, d) in runs {
            fill_group(&mut self.body, c, &[format!("<path d=\"{d}\"/>")]);
        }
    }

    /// A marker definition. The stroke is GLMakie's outer stroke: for opaque fills a centered
    /// stroke of twice the width painted below the fill (`paint-order`); for translucent fills
    /// the stroke is masked to the outside of the shape so it does not show through. Round
    /// joins match the offset of the GPU's signed distance field.
    fn marker_def(
        &mut self,
        marker: Marker,
        size: f64,
        rotation: f64,
        stroke: Color,
        sw: f64,
        translucent: bool,
    ) -> u32 {
        let outer_only = sw > 0.0 && translucent;
        let key = MarkerKey {
            marker,
            size: (size * 1000.0).round() as i64,
            rotation: (rotation * 1e6).round() as i64,
            stroke: if sw > 0.0 { stroke.to_rgba8() } else { [0; 4] },
            stroke_width: (sw * 1000.0).round() as i64,
            outer_only,
        };
        if let Some(id) = self.markers.get(&key) {
            return *id;
        }
        let id = self.next_id();
        self.markers.insert(key, id);
        let sh = marker::shape(marker);
        // The shape element without id, paint or closing: `<circle r=".."` or `<path d=".."`.
        let elem = match &sh {
            marker::Shape::Circle(r) => format!("<circle r=\"{}\"", N(r * size)),
            marker::Shape::Polygon(p) => {
                let mut e = String::from("<path d=\"");
                for (i, v) in marker::polygon_units(p, size, rotation).iter().enumerate() {
                    let _ = write!(e, "{}{} {}", if i == 0 { 'M' } else { 'L' }, N(v[0]), N(v[1]));
                }
                e.push_str("Z\"");
                e
            }
        };
        let mut stroke_attrs = String::new();
        if sw > 0.0 {
            paint(&mut stroke_attrs, "stroke", stroke);
            let _ = write!(stroke_attrs, " stroke-width=\"{}\" stroke-linejoin=\"round\"", N(2.0 * sw));
        }
        let d = &mut self.defs;
        if outer_only {
            let r = marker::bounding_radius(&sh) * size + 2.0 * sw + 1.0;
            let _ = write!(d, "<mask id=\"k{id}\" maskUnits=\"userSpaceOnUse\"");
            rect_attrs(d, -r, -r, 2.0 * r, 2.0 * r);
            d.push_str("><rect");
            rect_attrs(d, -r, -r, 2.0 * r, 2.0 * r);
            let _ = writeln!(d, " fill=\"#ffffff\"/>{elem} fill=\"#000000\"/></mask>");
            let _ =
                writeln!(d, "<g id=\"m{id}\">{elem} fill=\"none\"{stroke_attrs} mask=\"url(#k{id})\"/>{elem}/></g>");
        } else {
            let tail = elem.strip_prefix('<').unwrap_or(&elem);
            let (tag, rest) = tail.split_once(' ').unwrap_or((tail, ""));
            let _ = write!(d, "<{tag} id=\"m{id}\" {rest}{stroke_attrs}");
            if sw > 0.0 {
                d.push_str(" paint-order=\"stroke\"");
            }
            d.push_str("/>\n");
        }
        id
    }

    fn markers(&mut self, m: &MarkersPrim, xf: [f64; 4], clip: Option<Rect>) {
        let sw = if m.stroke_color.a > 0.0 { m.stroke_width.max(0.0) as f64 } else { 0.0 };
        let mapper = match &m.color {
            PrimColor::Values(_, map) => Some(field::Mapper::new(map)),
            _ => None,
        };
        let color_at = |i: usize| -> Option<Color> {
            match &m.color {
                PrimColor::Uniform(c) => Some(*c),
                PrimColor::PerElement(b) => b.data.get(i).map(|v| unpremul_u32(*v)),
                PrimColor::Values(b, _) => b.data.get(i).zip(mapper.as_ref()).map(|(v, mp)| mp.color(*v)),
            }
        };
        let reach = marker::bounding_radius(&marker::shape(m.marker));
        let uniform = matches!(m.color, PrimColor::Uniform(_));
        let mut uses: Vec<String> = Vec::new();
        for (i, p) in m.pos.data.iter().enumerate() {
            let Some(q) = map(xf, *p) else { continue };
            let size = m.sizes.as_ref().map_or(Some(m.size), |s| s.data.get(i).copied());
            let (Some(size), Some(fill)) = (size, color_at(i)) else { continue };
            let size = size as f64;
            if size.is_nan() || size <= 0.0 || (fill.a <= 0.0 && sw == 0.0) {
                continue;
            }
            if let Some(c) = clip {
                let r = reach * size + sw;
                if q[0] + r < c.x || q[0] - r > c.right() || q[1] + r < c.y || q[1] - r > c.bottom() {
                    continue;
                }
            }
            let id = self.marker_def(m.marker, size, m.rotation as f64, m.stroke_color, sw, fill.a < 1.0);
            let mut s = format!("<use xlink:href=\"#m{id}\" x=\"{}\" y=\"{}\"", N(q[0]), N(q[1]));
            if !uniform {
                paint(&mut s, "fill", fill);
            }
            s.push_str("/>");
            uses.push(s);
        }
        match &m.color {
            PrimColor::Uniform(c) => fill_group(&mut self.body, *c, &uses),
            _ => {
                for u in uses {
                    self.body.push_str(&u);
                    self.body.push('\n');
                }
            }
        }
    }

    fn gradient(&mut self, a: [f64; 2], b: [f64; 2], ca: Color, cb: Color) -> u32 {
        let id = self.next_id();
        let d = &mut self.defs;
        let _ = write!(
            d,
            "<linearGradient id=\"l{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">",
            N(a[0]),
            N(a[1]),
            N(b[0]),
            N(b[1])
        );
        for (off, c) in [(0, ca), (1, cb)] {
            let _ = write!(d, "<stop offset=\"{off}\"");
            paint(d, "stop-color", c);
            d.push_str("/>");
        }
        d.push_str("</linearGradient>\n");
        id
    }

    /// Polylines with NaN breaks or independent segments. Per-point colors follow CairoMakie:
    /// runs of equal color are one path, color changes become per-segment gradient strokes.
    fn lines(&mut self, l: &LinesPrim, xf: [f64; 4]) {
        let w = l.width as f64;
        if w.is_nan() || w <= 0.0 || l.pts.data.len() < 2 {
            return;
        }
        let pts: Vec<Option<[f64; 2]>> = l.pts.data.iter().map(|p| map(xf, *p)).collect();
        let mapper = match &l.color {
            PrimColor::Values(_, map) => Some(field::Mapper::new(map)),
            _ => None,
        };
        let colors: Option<Vec<Color>> = match &l.color {
            PrimColor::Uniform(_) => None,
            PrimColor::PerElement(b) => {
                Some((0..pts.len()).map(|i| b.data.get(i).map_or(Color::TRANSPARENT, |v| unpremul_u32(*v))).collect())
            }
            PrimColor::Values(b, _) => Some(
                (0..pts.len())
                    .map(|i| match (b.data.get(i), &mapper) {
                        (Some(v), Some(mp)) => mp.color(*v),
                        _ => Color::TRANSPARENT,
                    })
                    .collect(),
            ),
        };
        if let PrimColor::Uniform(c) = l.color
            && c.a <= 0.0
        {
            return;
        }

        let mut g = format!(" fill=\"none\" stroke-width=\"{}\"", N(w));
        let cap = match l.cap {
            LineCap::Butt => "butt",
            LineCap::Square => "square",
            LineCap::Round => "round",
        };
        if cap != "butt" {
            let _ = write!(g, " stroke-linecap=\"{cap}\"");
        }
        if !l.segments {
            match l.join {
                JoinStyle::Miter => {
                    // Makie's miter_limit is an angle; SVG's is the miter length ratio
                    // 1 / sin(angle / 2) (CairoMakie's `2 * miter_angle_to_distance`).
                    let a = l.miter_limit as f64;
                    let a = if a > 0.0 && a < std::f64::consts::PI { a } else { std::f64::consts::FRAC_PI_3 };
                    let _ = write!(g, " stroke-miterlimit=\"{}\"", N(1.0 / (0.5 * a).sin()));
                }
                JoinStyle::Bevel => g.push_str(" stroke-linejoin=\"bevel\""),
                JoinStyle::Round => g.push_str(" stroke-linejoin=\"round\""),
            }
        }
        if let Some(p) = &l.pattern {
            let dashes: Vec<f64> = p.windows(2).map(|d| ((d[1] - d[0]) as f64 * w).max(0.0)).collect();
            if dashes.iter().any(|d| *d > 0.0) {
                g.push_str(" stroke-dasharray=\"");
                for (i, d) in dashes.iter().enumerate() {
                    let _ = write!(g, "{}{}", if i == 0 { "" } else { " " }, N(*d));
                }
                g.push('"');
            }
        }

        // (d, color, gradient id)
        let mut paths: Vec<(String, Color, Option<u32>)> = Vec::new();
        let seg = |a: [f64; 2], b: [f64; 2]| format!("M{} {}L{} {}", N(a[0]), N(a[1]), N(b[0]), N(b[1]));
        match (&colors, l.segments) {
            (None, true) => {
                let mut d = String::new();
                for s in pts.as_chunks::<2>().0 {
                    if let (Some(a), Some(b)) = (s[0], s[1]) {
                        d.push_str(&seg(a, b));
                    }
                }
                let PrimColor::Uniform(c) = l.color else { return };
                paths.push((d, c, None));
            }
            (None, false) => {
                let PrimColor::Uniform(c) = l.color else { return };
                paths.push((polyline_d(&pts), c, None));
            }
            (Some(cs), true) => {
                for (k, s) in pts.as_chunks::<2>().0.iter().enumerate() {
                    if let (Some(a), Some(b)) = (s[0], s[1]) {
                        let (ca, cb) = (cs[2 * k], cs[2 * k + 1]);
                        let gid = (ca != cb).then(|| self.gradient(a, b, ca, cb));
                        paths.push((seg(a, b), ca, gid));
                    }
                }
            }
            (Some(cs), false) => {
                let mut run: Vec<Option<[f64; 2]>> = Vec::new();
                let mut run_color = Color::TRANSPARENT;
                let flush =
                    |run: &mut Vec<Option<[f64; 2]>>, c: Color, paths: &mut Vec<(String, Color, Option<u32>)>| {
                        if run.len() >= 2 {
                            paths.push((polyline_d(run), c, None));
                        }
                        run.clear();
                    };
                for i in 0..pts.len() - 1 {
                    let (Some(a), Some(b)) = (pts[i], pts[i + 1]) else {
                        flush(&mut run, run_color, &mut paths);
                        continue;
                    };
                    let (ca, cb) = (cs[i], cs[i + 1]);
                    if ca == cb {
                        if !(run_color == ca && run.last() == Some(&Some(a))) {
                            flush(&mut run, run_color, &mut paths);
                            run.push(Some(a));
                            run_color = ca;
                        }
                        run.push(Some(b));
                    } else {
                        flush(&mut run, run_color, &mut paths);
                        let gid = self.gradient(a, b, ca, cb);
                        paths.push((seg(a, b), ca, Some(gid)));
                    }
                }
                flush(&mut run, run_color, &mut paths);
            }
        }
        paths.retain(|(d, c, gid)| !d.is_empty() && (gid.is_some() || c.a > 0.0));
        if paths.is_empty() {
            return;
        }
        let one = |p: &(String, Color, Option<u32>)| {
            let mut s = format!("<path d=\"{}\"", p.0);
            match p.2 {
                Some(id) => {
                    let _ = write!(s, " stroke=\"url(#l{id})\"");
                }
                None => paint(&mut s, "stroke", p.1),
            }
            s
        };
        if let [p] = paths.as_slice() {
            let _ = writeln!(self.body, "{}{g}/>", one(p));
        } else {
            let _ = writeln!(self.body, "<g{g}>");
            for p in &paths {
                let _ = writeln!(self.body, "{}/>", one(p));
            }
            self.body.push_str("</g>\n");
        }
    }

    fn glyph_def(&mut self, font: Font, glyph: u16) -> Option<u32> {
        if let Some(id) = self.glyphs.get(&(font, glyph)) {
            return *id;
        }
        let id = glyph_path(font, glyph).map(|segs| {
            let id = self.next_id();
            let d = &mut self.defs;
            let _ = write!(d, "<path id=\"g{id}\" d=\"");
            // In em units: 3 decimals are exact for 1000-unit fonts.
            let upem = units_per_em(font);
            let pt = |d: &mut String, p: [f32; 2]| {
                let _ = write!(d, "{} {}", N(p[0] as f64 / upem), N(p[1] as f64 / upem));
            };
            for s in segs {
                match s {
                    PathSeg::Move(p) => {
                        d.push('M');
                        pt(d, p);
                    }
                    PathSeg::Line(p) => {
                        d.push('L');
                        pt(d, p);
                    }
                    PathSeg::Quad(c, p) => {
                        d.push('Q');
                        pt(d, c);
                        d.push(' ');
                        pt(d, p);
                    }
                    PathSeg::Cubic(c1, c2, p) => {
                        d.push('C');
                        pt(d, c1);
                        d.push(' ');
                        pt(d, c2);
                        d.push(' ');
                        pt(d, p);
                    }
                    PathSeg::Close => d.push('Z'),
                }
            }
            d.push_str("\"/>\n");
            id
        });
        self.glyphs.insert((font, glyph), id);
        id
    }

    /// Glyph outlines (em units) referenced per glyph as Cairo does: `translate · rotate · scale(size)`.
    fn glyphs(&mut self, gp: &GlyphsPrim, xf: [f64; 4]) {
        let mut runs: Vec<(Color, Vec<String>)> = Vec::new();
        for g in &gp.glyphs {
            if g.color.a <= 0.0 || g.size.is_nan() || g.size <= 0.0 {
                continue;
            }
            let Some(p) = map(xf, g.pos) else { continue };
            let Some(id) = self.glyph_def(g.font, g.glyph) else { continue };
            let mut s = format!("<use xlink:href=\"#g{id}\" transform=\"translate({} {})", N(p[0]), N(p[1]));
            let deg = -(g.angle as f64).to_degrees();
            if (deg * 1000.0).round() != 0.0 {
                let _ = write!(s, " rotate({})", N(deg));
            }
            // Outlines are in em units, so the scale is the font size.
            let _ = write!(s, " scale({})\"/>", N(g.size as f64));
            match runs.last_mut() {
                Some((c, v)) if *c == g.color => v.push(s),
                _ => runs.push((g.color, vec![s])),
            }
        }
        for (c, v) in runs {
            fill_group(&mut self.body, c, &v);
        }
    }

    /// Heatmaps: a regular grid is an embedded PNG (one pixel per cell, integer-upsampled),
    /// an irregular one is run-length merged `<rect>`s.
    fn field(&mut self, f: &FieldPrim, xf: [f64; 4]) {
        if let Some(img) = regular_image(f, xf) {
            let k = field::upsample_factor(&img, f.interpolate);
            let Some(png) = field::encode_png(&img, k) else { return };
            let [x, y, w, h] = img.rect;
            let _ = write!(self.body, "<image");
            rect_attrs(&mut self.body, x, y, w, h);
            self.body.push_str(" preserveAspectRatio=\"none\"");
            if !f.interpolate {
                self.body.push_str(" image-rendering=\"pixelated\" style=\"image-rendering:optimizeSpeed\"");
            }
            let _ = writeln!(self.body, " xlink:href=\"data:image/png;base64,{}\"/>", BASE64_STANDARD.encode(png));
            return;
        }
        let (Some(ex), Some(ey), Some(colors)) =
            (field::edges(&f.x, f.nx), field::edges(&f.y, f.ny), field::cell_colors(f))
        else {
            return;
        };
        let fx: Vec<f64> = ex.iter().map(|v| v * xf[0] + xf[2]).collect();
        let fy: Vec<f64> = ey.iter().map(|v| v * xf[1] + xf[3]).collect();
        let nx = f.nx as usize;
        let mut out = String::from("<g shape-rendering=\"crispEdges\">\n");
        for j in 0..f.ny as usize {
            let (y0, y1) = (fy[j], fy[j + 1]);
            if !(y0.is_finite() && y1.is_finite()) || y0 == y1 {
                continue;
            }
            let mut i = 0;
            while i < nx {
                let c = colors[j * nx + i];
                let mut e = i + 1;
                while e < nx && colors[j * nx + e] == c {
                    e += 1;
                }
                let (x0, x1) = (fx[i], fx[e]);
                if c.a > 0.0 && x0.is_finite() && x1.is_finite() && x0 != x1 {
                    out.push_str("<rect");
                    rect_attrs(&mut out, x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
                    paint(&mut out, "fill", c);
                    out.push_str("/>\n");
                }
                i = e;
            }
        }
        out.push_str("</g>\n");
        self.body.push_str(&out);
    }

    fn finish(mut self) -> String {
        self.set_clip(None);
        let [w, h] = self.dl.size;
        let pt = self.opts.pt_per_unit;
        let mut s = String::with_capacity(self.defs.len() + self.body.len() + 512);
        s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let _ = writeln!(
            s,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" version=\"1.1\" \
             width=\"{}pt\" height=\"{}pt\" viewBox=\"0 0 {} {}\">",
            N(w * pt),
            N(h * pt),
            N(w),
            N(h)
        );
        if !self.defs.is_empty() {
            s.push_str("<defs>\n");
            s.push_str(&self.defs);
            s.push_str("</defs>\n");
        }
        let bg = self.dl.background;
        if bg.a > 0.0 {
            let _ = write!(s, "<rect width=\"{}\" height=\"{}\"", N(w), N(h));
            paint(&mut s, "fill", bg);
            s.push_str("/>\n");
        }
        s.push_str(&self.body);
        s.push_str("</svg>\n");
        s
    }
}

/// Path data for a polyline with `None` breaks; a subpath ending where it started is closed.
fn polyline_d(pts: &[Option<[f64; 2]>]) -> String {
    let mut d = String::new();
    let mut i = 0;
    while i < pts.len() {
        let Some(start) = pts[i] else {
            i += 1;
            continue;
        };
        let mut e = i + 1;
        while e < pts.len() && pts[e].is_some() {
            e += 1;
        }
        let run: Vec<[f64; 2]> = pts[i..e].iter().flatten().copied().collect();
        if run.len() >= 2 {
            let _ = write!(d, "M{} {}", N(start[0]), N(start[1]));
            let closed = run.len() >= 3 && run[run.len() - 1] == start;
            let end = if closed { run.len() - 1 } else { run.len() };
            for p in &run[1..end] {
                let _ = write!(d, "L{} {}", N(p[0]), N(p[1]));
            }
            if closed {
                d.push('Z');
            }
        }
        i = e;
    }
    d
}
