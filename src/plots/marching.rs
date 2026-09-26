//! Marching squares on rectilinear grids, shared by `contour` (isolines) and `contourf` (isobands).
//!
//! Isolines follow Contour.jl (what Makie's `contour` uses): a corner is above the level when
//! `z > h`, and a saddle cell joins its two high corners when the mean of its corners is `>= h`.
//! Segments are linked through the grid edges they cross into polylines; a line that returns to
//! its start is closed (its last point repeats the first).
//!
//! Isobands follow Isoband.jl (what Makie's `contourf` uses): a corner is inside the band
//! `[lo, hi)` when `lo <= z < hi`; saddles are resolved per threshold with the cell's mean value,
//! consistently with the isolines. Each band is returned as triangles: cells entirely inside the
//! band are merged into one quad per row run, cells the band boundary crosses are cut into convex
//! pieces and fanned. Every crossing point is computed from its grid edge alone, so neighbouring
//! cells and neighbouring bands share bit-identical vertices and the mesh has no cracks (seamless
//! under MSAA; merged per band in SVG). Cells with a NaN corner are left out.

use crate::color::ValueEncoding;
use crate::data::{CellSpecKind as Spec, Data2D};
use std::collections::HashMap;
use std::sync::Arc;

/// Grid point coordinates along one axis for `n` points (Makie's `VertexGrid` conversion):
/// no coordinates give `1..=n`, `a..=b` and `Edges(a, b)` give `n` points from `a` to `b`, and a
/// vector must hold `n` values.
pub(crate) fn vertex_coords(spec: &Spec, n: usize) -> Result<Vec<f64>, String> {
    let lin = |a: f64, b: f64| -> Vec<f64> {
        if n == 1 { vec![a] } else { (0..n).map(|i| a + (b - a) * (i as f64 / (n - 1) as f64)).collect() }
    };
    Ok(match spec {
        Spec::Index => (1..=n).map(|i| i as f64).collect(),
        Spec::Centres(a, b) | Spec::Outer(a, b) => lin(*a, *b),
        Spec::Values(v) if v.len() == n => v.as_ref().clone(),
        Spec::Values(v) => return Err(format!("{} coordinates for {n} grid points; expected {n}", v.len())),
    })
}

/// A 2D field on grid points, as `contour` and `contourf` store it: coordinates in f64, values
/// encoded to f32 like the heatmap's (`(v - off) * k`, so any magnitude keeps its resolution).
#[derive(Clone, Debug)]
pub(crate) struct GridField {
    pub xspec: Spec,
    pub yspec: Spec,
    pub x: Arc<Vec<f64>>,
    pub y: Arc<Vec<f64>>,
    /// `nx * ny` encoded values, x fastest.
    pub values: Arc<Vec<f32>>,
    pub enc: ValueEncoding,
    /// Unique per data/coordinate change (keys derived geometry).
    pub generation: u64,
}

/// Encoded values of `z` (conversion off the figure lock).
pub(crate) fn encode(z: &impl Data2D) -> (Vec<f32>, ValueEncoding) {
    let enc = ValueEncoding::new(z.extrema());
    let mut v = Vec::new();
    z.write_f32(&mut v, enc.off, enc.k);
    (v, enc)
}

impl GridField {
    pub fn new(xspec: Spec, yspec: Spec, z: &impl Data2D) -> Result<GridField, String> {
        let (nx, ny) = z.dims();
        let x = vertex_coords(&xspec, nx).map_err(|e| format!("x: {e}"))?;
        let y = vertex_coords(&yspec, ny).map_err(|e| format!("y: {e}"))?;
        let (values, enc) = encode(z);
        Ok(GridField {
            xspec,
            yspec,
            x: Arc::new(x),
            y: Arc::new(y),
            values: Arc::new(values),
            enc,
            generation: crate::figure::next_uid(),
        })
    }

    /// Replaces the values; new dimensions recompute the coordinates from their specs.
    pub fn set_values(&mut self, dims: (usize, usize), values: Vec<f32>, enc: ValueEncoding) -> Result<(), String> {
        if dims != (self.x.len(), self.y.len()) {
            self.x = Arc::new(vertex_coords(&self.xspec, dims.0).map_err(|e| format!("x: {e}"))?);
            self.y = Arc::new(vertex_coords(&self.yspec, dims.1).map_err(|e| format!("y: {e}"))?);
        }
        self.values = Arc::new(values);
        self.enc = enc;
        self.generation = crate::figure::next_uid();
        Ok(())
    }

    /// Replaces the coordinate specs.
    pub fn set_coords(&mut self, xspec: Spec, yspec: Spec) -> Result<(), String> {
        let x = vertex_coords(&xspec, self.x.len()).map_err(|e| format!("x: {e}"))?;
        let y = vertex_coords(&yspec, self.y.len()).map_err(|e| format!("y: {e}"))?;
        (self.x, self.y, self.xspec, self.yspec) = (Arc::new(x), Arc::new(y), xspec, yspec);
        self.generation = crate::figure::next_uid();
        Ok(())
    }

    /// The grid in encoded value space (encode levels with [`GridField::level`]).
    pub fn grid(&self) -> Grid<'_, f32> {
        Grid { x: &self.x, y: &self.y, z: &self.values }
    }

    /// A level in the encoded value space (infinities stay infinite).
    pub fn level(&self, v: f64) -> f64 {
        (v - self.enc.off) * self.enc.k
    }

    /// Finite extrema of the values (Makie's `nan_extrema`).
    pub fn zrange(&self) -> Option<(f64, f64)> {
        self.enc.extrema
    }

    /// Finite bounds of the grid points in scaled space (Makie's contour `data_limits`).
    pub fn bounds(&self, xs: crate::transform::Scale, ys: crate::transform::Scale) -> Option<[f64; 4]> {
        let (x0, x1) = crate::data::finite_extrema(self.x.iter().map(|v| xs.forward(*v)))?;
        let (y0, y1) = crate::data::finite_extrema(self.y.iter().map(|v| ys.forward(*v)))?;
        Some([x0, x1, y0, y1])
    }

    /// The cell `(i, j)` containing the data point `p`, for monotone coordinates.
    pub fn cell_at(&self, p: [f64; 2]) -> Option<(usize, usize)> {
        let find = |c: &[f64], v: f64| (0..c.len().saturating_sub(1)).find(|&i| (c[i] - v) * (c[i + 1] - v) <= 0.0);
        Some((find(&self.x, p[0])?, find(&self.y, p[1])?))
    }
}

/// `true` if `p` lies in the band `lo <= z < hi` inside cell `(i, j)` (exactly the region
/// [`isoband`] fills there).
pub(crate) fn band_contains<T: Copy + Into<f64>>(
    g: &Grid<'_, T>,
    (i, j): (usize, usize),
    lo: f64,
    hi: f64,
    p: [f64; 2],
) -> bool {
    if !g.valid() || i + 1 >= g.nx() || j + 1 >= g.ny() || lo.is_nan() || hi.is_nan() || lo >= hi {
        return false;
    }
    let Some(c) = g.corners(i, j) else { return false };
    let cls = c.map(|v| {
        if v < lo {
            0u8
        } else if v < hi {
            1
        } else {
            2
        }
    });
    let mut t = Vec::new();
    if cls == [1; 4] {
        let (x0, x1, y0, y1) = (g.x[i], g.x[i + 1], g.y[j], g.y[j + 1]);
        t.extend_from_slice(&[[x0, y0], [x1, y0], [x1, y1], [x0, y0], [x1, y1], [x0, y1]]);
    } else if cls != [0; 4] && cls != [2; 4] {
        band_cell(g, i, j, c, cls, lo, hi, &mut t);
    }
    let side = |a: [f64; 2], b: [f64; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    t.chunks(3).any(|t| {
        let (d0, d1, d2) = (side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0]));
        (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
    })
}

/// A rectilinear grid of values: `z[j * nx + i]` sits at `(x[i], y[j])`.
pub(crate) struct Grid<'a, T> {
    pub x: &'a [f64],
    pub y: &'a [f64],
    pub z: &'a [T],
}

/// One contour line. A closed line's last point repeats its first.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Polyline {
    pub pts: Vec<[f64; 2]>,
    pub closed: bool,
}

/// Grid edge key: `2 * (j * nx + i)` for the horizontal edge `(i, j)-(i + 1, j)`, plus one for the
/// vertical edge `(i, j)-(i, j + 1)`.
type EdgeKey = u64;

impl<T: Copy + Into<f64>> Grid<'_, T> {
    fn nx(&self) -> usize {
        self.x.len()
    }

    fn ny(&self) -> usize {
        self.y.len()
    }

    /// `true` if the grid has at least one cell and `z` matches `x` and `y`.
    fn valid(&self) -> bool {
        self.nx() >= 2 && self.ny() >= 2 && self.z.len() == self.nx() * self.ny()
    }

    #[inline]
    fn z(&self, i: usize, j: usize) -> f64 {
        self.z[j * self.nx() + i].into()
    }

    fn hkey(&self, i: usize, j: usize) -> EdgeKey {
        2 * (j * self.nx() + i) as u64
    }

    fn vkey(&self, i: usize, j: usize) -> EdgeKey {
        2 * (j * self.nx() + i) as u64 + 1
    }

    /// Where level `h` crosses edge `key`, interpolated from the edge's lower-index end (Contour.jl's
    /// `interpolate`), so every cell sharing the edge computes the same point.
    fn crossing(&self, key: EdgeKey, h: f64) -> [f64; 2] {
        let k = (key / 2) as usize;
        let (i, j) = (k % self.nx(), k / self.nx());
        if key.is_multiple_of(2) {
            let (za, zb) = (self.z(i, j), self.z(i + 1, j));
            [self.x[i] + (self.x[i + 1] - self.x[i]) * (h - za) / (zb - za), self.y[j]]
        } else {
            let (za, zb) = (self.z(i, j), self.z(i, j + 1));
            [self.x[i], self.y[j] + (self.y[j + 1] - self.y[j]) * (h - za) / (zb - za)]
        }
    }

    /// Corner values of cell `(i, j)` counter-clockwise from the lower-left: `[SW, SE, NE, NW]`,
    /// or `None` if one is NaN.
    fn corners(&self, i: usize, j: usize) -> Option<[f64; 4]> {
        let c = [self.z(i, j), self.z(i + 1, j), self.z(i + 1, j + 1), self.z(i, j + 1)];
        (!c.iter().any(|v| v.is_nan())).then_some(c)
    }
}

/// The contour lines of `g` at level `h`.
pub(crate) fn isolines<T: Copy + Into<f64>>(g: &Grid<'_, T>, h: f64) -> Vec<Polyline> {
    if !g.valid() || h.is_nan() {
        return Vec::new();
    }
    let mut segs: Vec<[EdgeKey; 2]> = Vec::new();
    for j in 0..g.ny() - 1 {
        for i in 0..g.nx() - 1 {
            let Some(c) = g.corners(i, j) else { continue };
            let up = c.map(|v| v > h);
            let case = up[0] as u8 | (up[1] as u8) << 1 | (up[2] as u8) << 2 | (up[3] as u8) << 3;
            let (s, e, n, w) = (g.hkey(i, j), g.vkey(i + 1, j), g.hkey(i, j + 1), g.vkey(i, j));
            let high_center = 0.25 * (c[0] + c[1] + c[2] + c[3]) >= h;
            match case {
                0 | 15 => {}
                // SW and NE high: joined (N-W, S-E cut off the low corners) or separate.
                5 => segs.extend(if high_center { [[n, w], [s, e]] } else { [[n, e], [s, w]] }),
                // SE and NW high.
                10 => segs.extend(if high_center { [[n, e], [s, w]] } else { [[n, w], [s, e]] }),
                _ => {
                    let mut cross = [s, e, n, w].into_iter().zip(0..4).filter(|(_, k)| up[*k] != up[(*k + 1) % 4]);
                    if let (Some((a, _)), Some((b, _))) = (cross.next(), cross.next()) {
                        segs.push([a, b]);
                    }
                }
            }
        }
    }
    link(&segs)
        .into_iter()
        .map(|(edges, closed)| Polyline { pts: edges.iter().map(|&k| g.crossing(k, h)).collect(), closed })
        .collect()
}

/// Joins segments that share a grid edge into chains of edges; `true` marks closed chains (their
/// last edge repeats the first).
fn link(segs: &[[EdgeKey; 2]]) -> Vec<(Vec<EdgeKey>, bool)> {
    const NONE: u32 = u32::MAX;
    let mut at: HashMap<EdgeKey, [u32; 2]> = HashMap::with_capacity(segs.len() * 2);
    for (k, s) in segs.iter().enumerate() {
        for e in s {
            let slot = at.entry(*e).or_insert([NONE; 2]);
            if slot[0] == NONE {
                slot[0] = k as u32;
            } else {
                slot[1] = k as u32;
            }
        }
    }
    let degree = |e: EdgeKey| at.get(&e).map_or(0, |s| s.iter().filter(|k| **k != NONE).count());
    let next = |e: EdgeKey, cur: u32| -> Option<u32> {
        let s = at.get(&e)?;
        let o = if s[0] == cur { s[1] } else { s[0] };
        (o != NONE && o != cur).then_some(o)
    };
    let mut visited = vec![false; segs.len()];
    let walk = |start: usize, entry: EdgeKey, visited: &mut Vec<bool>| -> Vec<EdgeKey> {
        let mut chain = vec![entry];
        let (mut cur, mut e) = (start as u32, entry);
        loop {
            visited[cur as usize] = true;
            let s = segs[cur as usize];
            e = if s[0] == e { s[1] } else { s[0] };
            chain.push(e);
            match next(e, cur) {
                Some(n) if !visited[n as usize] => cur = n,
                _ => break,
            }
        }
        chain
    };
    let mut out = Vec::new();
    // Open lines start at an edge only one segment crosses (the grid boundary or a NaN cell).
    for k in 0..segs.len() {
        if visited[k] {
            continue;
        }
        if let Some(&e) = segs[k].iter().find(|e| degree(**e) == 1) {
            out.push((walk(k, e, &mut visited), false));
        }
    }
    // Everything left is a closed loop.
    for k in 0..segs.len() {
        if !visited[k] {
            let chain = walk(k, segs[k][0], &mut visited);
            let closed = chain.len() >= 3 && chain.first() == chain.last();
            out.push((chain, closed));
        }
    }
    out
}

/// A point on a cell's boundary ring: a corner (`thr = None`) or a band-threshold crossing.
#[derive(Clone, Copy, Default)]
struct RingPt {
    pos: [f64; 2],
    /// `Some(false)`: crosses `lo`; `Some(true)`: crosses `hi`.
    thr: Option<bool>,
    /// Class of the ring just after this point (0 below `lo`, 1 in the band, 2 at or above `hi`).
    after: u8,
}

/// Appends the triangles (3 points each) covering `lo <= z < hi` (either bound may be infinite).
pub(crate) fn isoband<T: Copy + Into<f64>>(g: &Grid<'_, T>, lo: f64, hi: f64, out: &mut Vec<[f64; 2]>) {
    if !g.valid() || lo.is_nan() || hi.is_nan() || lo >= hi {
        return;
    }
    let class = |v: f64| {
        if v < lo {
            0u8
        } else if v < hi {
            1
        } else {
            2
        }
    };
    let quad = |out: &mut Vec<[f64; 2]>, i0: usize, i1: usize, j: usize| {
        let (x0, x1, y0, y1) = (g.x[i0], g.x[i1], g.y[j], g.y[j + 1]);
        out.extend_from_slice(&[[x0, y0], [x1, y0], [x1, y1], [x0, y0], [x1, y1], [x0, y1]]);
    };
    for j in 0..g.ny() - 1 {
        let mut run: Option<usize> = None;
        for i in 0..g.nx() - 1 {
            let c = g.corners(i, j);
            let cls = c.map(|c| c.map(class));
            if cls == Some([1; 4]) {
                run.get_or_insert(i);
                continue;
            }
            if let Some(i0) = run.take() {
                quad(out, i0, i, j);
            }
            if let (Some(c), Some(cls)) = (c, cls)
                && cls != [0; 4]
                && cls != [2; 4]
            {
                band_cell(g, i, j, c, cls, lo, hi, out);
            }
        }
        if let Some(i0) = run {
            quad(out, i0, g.nx() - 1, j);
        }
    }
}

/// The band's pieces in one cell the band boundary crosses, fanned into triangles.
#[allow(clippy::too_many_arguments)]
fn band_cell<T: Copy + Into<f64>>(
    g: &Grid<'_, T>,
    i: usize,
    j: usize,
    c: [f64; 4],
    cls: [u8; 4],
    lo: f64,
    hi: f64,
    out: &mut Vec<[f64; 2]>,
) {
    // The boundary ring, counter-clockwise from SW: corners with the crossings between them in
    // the order they are met.
    let corner_pos = [[g.x[i], g.y[j]], [g.x[i + 1], g.y[j]], [g.x[i + 1], g.y[j + 1]], [g.x[i], g.y[j + 1]]];
    let edges = [g.hkey(i, j), g.vkey(i + 1, j), g.hkey(i, j + 1), g.vkey(i, j)];
    let mut ring = [RingPt::default(); 12];
    let mut n = 0;
    let mut push = |p: RingPt| {
        ring[n] = p;
        n += 1;
    };
    for k in 0..4 {
        let (a, b) = (cls[k], cls[(k + 1) % 4]);
        push(RingPt { pos: corner_pos[k], thr: None, after: a });
        let cross = |hi_thr: bool, after: u8| RingPt {
            pos: g.crossing(edges[k], if hi_thr { hi } else { lo }),
            thr: Some(hi_thr),
            after,
        };
        if a < b {
            if a == 0 {
                push(cross(false, 1));
            }
            if b == 2 {
                push(cross(true, 2));
            }
        } else if a > b {
            if a == 2 {
                push(cross(true, 1));
            }
            if b == 0 {
                push(cross(false, 0));
            }
        }
    }
    let ring = &ring[..n];

    // Pair the crossings of each threshold with chords (marching squares per threshold).
    const NONE: usize = usize::MAX;
    let mut partner = [NONE; 12];
    let center = 0.25 * (c[0] + c[1] + c[2] + c[3]);
    for hi_thr in [false, true] {
        let mut cs = [0usize; 4];
        let mut m = 0;
        for (k, p) in ring.iter().enumerate() {
            if p.thr == Some(hi_thr) {
                if m == 4 {
                    return;
                }
                cs[m] = k;
                m += 1;
            }
        }
        let pairs: [(usize, usize); 2] = match m {
            0 => continue,
            2 => [(cs[0], cs[1]), (NONE, NONE)],
            4 => {
                // Is the ring above this threshold between the first two crossings, and is the
                // cell's centre above it? A high centre joins the high corners: the chords then
                // cut off the low arcs.
                let above = |after: u8| if hi_thr { after == 2 } else { after >= 1 };
                let first_arc_above = above(ring[cs[0]].after);
                let center_above = if hi_thr { center >= hi } else { center >= lo };
                if first_arc_above == center_above {
                    [(cs[1], cs[2]), (cs[3], cs[0])]
                } else {
                    [(cs[0], cs[1]), (cs[2], cs[3])]
                }
            }
            _ => return,
        };
        for (a, b) in pairs {
            if a != NONE {
                partner[a] = b;
                partner[b] = a;
            }
        }
    }

    // Trace each piece: along the ring through in-band corners to the run's end crossing, then
    // along its chord to the start of the next run, until back at the first start.
    let is_start = |k: usize| ring[k].thr.is_some() && ring[k].after == 1;
    let mut used = [false; 12];
    let mut poly: Vec<[f64; 2]> = Vec::with_capacity(12);
    for s in 0..n {
        if !is_start(s) || used[s] {
            continue;
        }
        poly.clear();
        let mut cur = s;
        for _ in 0..n {
            used[cur] = true;
            poly.push(ring[cur].pos);
            let mut k = (cur + 1) % n;
            while ring[k].thr.is_none() && k != cur {
                poly.push(ring[k].pos);
                k = (k + 1) % n;
            }
            poly.push(ring[k].pos);
            let p = partner[k];
            if p == NONE || p == s || !is_start(p) || used[p] {
                break;
            }
            cur = p;
        }
        for t in 1..poly.len().saturating_sub(1) {
            out.extend_from_slice(&[poly[0], poly[t], poly[t + 1]]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid<'a>(x: &'a [f64], y: &'a [f64], z: &'a [f64]) -> Grid<'a, f64> {
        Grid { x, y, z }
    }

    fn area(tris: &[[f64; 2]]) -> f64 {
        tris.chunks(3)
            .map(|t| {
                0.5 * ((t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0])).abs()
            })
            .sum()
    }

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-12 && (a[1] - b[1]).abs() < 1e-12
    }

    /// A peak in the middle of a 3 x 3 grid gives one closed diamond.
    #[test]
    fn closed_loop_around_a_peak() {
        let (x, y) = ([0.0, 1.0, 2.0], [0.0, 1.0, 2.0]);
        let z = [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let l = isolines(&grid(&x, &y, &z), 0.5);
        assert_eq!(l.len(), 1);
        assert!(l[0].closed);
        assert_eq!(l[0].pts.len(), 5);
        assert!(close(l[0].pts[0], l[0].pts[4]));
        for p in &l[0].pts {
            assert!(((p[0] - 1.0).abs() + (p[1] - 1.0).abs() - 0.5).abs() < 1e-12, "{p:?} is not on the diamond");
        }
    }

    /// A ramp gives one open line from boundary to boundary, through every row.
    #[test]
    fn open_line_across_the_grid() {
        let x: Vec<f64> = (0..5).map(|i| i as f64).collect();
        let y = x.clone();
        let z: Vec<f64> = (0..25).map(|k| (k % 5) as f64).collect();
        let l = isolines(&grid(&x, &y, &z), 1.5);
        assert_eq!(l.len(), 1);
        assert!(!l[0].closed);
        assert_eq!(l[0].pts.len(), 5);
        assert!(l[0].pts.iter().all(|p| (p[0] - 1.5).abs() < 1e-12));
        let mut ys: Vec<f64> = l[0].pts.iter().map(|p| p[1]).collect();
        ys.sort_by(f64::total_cmp);
        assert_eq!(ys, [0.0, 1.0, 2.0, 3.0, 4.0]);
    }

    /// Contour.jl's saddle rule: a mean at or above the level joins the high corners.
    #[test]
    fn saddle_disambiguation() {
        let (x, y) = ([0.0, 1.0], [0.0, 1.0]);
        // Flat order SW, SE, NW, NE: SW and NE high, mean 0.5.
        let z = [1.0, 0.0, 0.0, 1.0];
        let seg = |h: f64| {
            let mut l = isolines(&grid(&x, &y, &z), h);
            l.sort_by(|a, b| a.pts[0][0].total_cmp(&b.pts[0][0]));
            l
        };
        // h = 0.4 < mean: joined; each line cuts off a low corner (SE or NW).
        for l in seg(0.4) {
            let mid = [(l.pts[0][0] + l.pts[1][0]) / 2.0, (l.pts[0][1] + l.pts[1][1]) / 2.0];
            assert!((mid[0] > 0.5) != (mid[1] > 0.5), "joined lines hug the low corners: {mid:?}");
        }
        // h = 0.6 > mean: separate; each line cuts off a high corner (SW or NE).
        let l = seg(0.6);
        assert_eq!(l.len(), 2);
        for l in l {
            let mid = [(l.pts[0][0] + l.pts[1][0]) / 2.0, (l.pts[0][1] + l.pts[1][1]) / 2.0];
            assert!((mid[0] > 0.5) == (mid[1] > 0.5), "separate lines hug the high corners: {mid:?}");
        }
    }

    /// NaN cells are skipped: a loop through them opens up.
    #[test]
    fn nan_cells_break_lines() {
        let (x, y) = ([0.0, 1.0, 2.0], [0.0, 1.0, 2.0]);
        let mut z = [0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        z[0] = f64::NAN;
        let l = isolines(&grid(&x, &y, &z), 0.5);
        assert_eq!(l.len(), 1);
        assert!(!l[0].closed);
        assert_eq!(l[0].pts.len(), 4);
        assert!(isolines(&grid(&x, &y, &z), f64::NAN).is_empty());
    }

    fn random_field(nx: usize, ny: usize, seed: u64) -> Vec<f64> {
        let mut s = seed;
        (0..nx * ny)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 33) % 7) as f64 / 3.0
            })
            .collect()
    }

    /// Bands partition the grid: their areas add up to the whole (no gaps, no overlaps), also on
    /// saddle-rich random data with values exactly on the levels, and splitting a band in two
    /// keeps its area.
    #[test]
    fn bands_partition_the_grid() {
        let (nx, ny) = (9, 7);
        let x: Vec<f64> = (0..nx).map(|i| (i as f64).powf(1.3)).collect();
        let y: Vec<f64> = (0..ny).map(|j| j as f64 * 0.5).collect();
        let total = (x[nx - 1] - x[0]) * (y[ny - 1] - y[0]);
        for seed in 1..20 {
            let z = random_field(nx, ny, seed);
            let g = grid(&x, &y, &z);
            let edges = [f64::NEG_INFINITY, 0.3, 2.0 / 3.0, 1.0, 1.5, f64::INFINITY];
            let mut sum = 0.0;
            for w in edges.windows(2) {
                let mut t = Vec::new();
                isoband(&g, w[0], w[1], &mut t);
                assert_eq!(t.len() % 3, 0);
                sum += area(&t);
            }
            assert!((sum - total).abs() < 1e-9, "seed {seed}: {sum} vs {total}");
            let (mut a, mut b, mut ab) = (Vec::new(), Vec::new(), Vec::new());
            isoband(&g, 0.3, 1.0, &mut a);
            isoband(&g, 1.0, 1.5, &mut b);
            isoband(&g, 0.3, 1.5, &mut ab);
            assert!((area(&a) + area(&b) - area(&ab)).abs() < 1e-9);
        }
    }

    /// A saddle cell with both thresholds crossing four times gives the octagon when the centre
    /// is in the band, and two pieces when it is not.
    #[test]
    fn band_saddles() {
        let (x, y) = ([0.0, 1.0], [0.0, 1.0]);
        // SW, SE, NE, NW alternate 0 / 2 (flat order SW, SE, NW, NE); band [0.5, 1.5); centre 1 is inside.
        let z = [0.0, 2.0, 2.0, 0.0];
        let mut t = Vec::new();
        isoband(&grid(&x, &y, &z), 0.5, 1.5, &mut t);
        assert_eq!(t.len(), 3 * 6, "one octagon");
        assert!((area(&t) - (1.0 - 4.0 * 0.5 * 0.25 * 0.25)).abs() < 1e-12);
        // Centre 0.4 below the band: the band's pieces hug the high corners, cut by `lo` chords.
        let z = [-0.4, 1.2, 1.2, -0.4];
        let mut t = Vec::new();
        isoband(&grid(&x, &y, &z), 0.5, 1.5, &mut t);
        assert_eq!(t.len(), 2 * 3, "two triangles");
    }

    /// A NaN corner leaves its cells out of every band.
    #[test]
    fn nan_cells_are_holes() {
        let (x, y) = ([0.0, 1.0, 2.0], [0.0, 1.0]);
        let z = [0.0, f64::NAN, 1.0, 0.0, 0.5, 1.0];
        let mut t = Vec::new();
        isoband(&grid(&x, &y, &z), f64::NEG_INFINITY, f64::INFINITY, &mut t);
        assert!(t.is_empty());
        let z = [0.0, 0.5, 1.0, 0.0, 0.5, f64::NAN];
        let mut t = Vec::new();
        isoband(&grid(&x, &y, &z), f64::NEG_INFINITY, f64::INFINITY, &mut t);
        assert!((area(&t) - 1.0).abs() < 1e-12);
    }

    /// Full cells merge into one quad per row run.
    #[test]
    fn full_rows_merge() {
        let x: Vec<f64> = (0..11).map(|i| i as f64).collect();
        let y = [0.0, 1.0, 2.0];
        let z = vec![0.5; 33];
        let mut t = Vec::new();
        isoband(&grid(&x, &y, &z), 0.0, 1.0, &mut t);
        assert_eq!(t.len(), 2 * 6);
        assert!((area(&t) - 20.0).abs() < 1e-12);
    }
}
