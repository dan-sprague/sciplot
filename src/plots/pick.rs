//! Hover picking for the data inspector: the plot element nearest to the cursor, found on the CPU
//! from the last frame's snapshot (never under the figure lock).

use crate::scene::AxisFrame;
use crate::theme::{Globals, Theme};
use crate::transform::Scale;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// What the inspector shows for a hovered element.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hover {
    /// Distance from the cursor in figure units (the nearest hover wins).
    pub dist: f64,
    /// The point the tooltip points at, in figure units.
    pub anchor: [f64; 2],
    /// Tooltip text (may contain `\n`).
    pub text: String,
    /// Diameter (units) of a highlight ring drawn around `anchor`, if any.
    pub ring: Option<f64>,
    /// A highlighted rectangle `[x, y, w, h]` in figure units (a heatmap cell), if any.
    pub outline: Option<[f64; 4]>,
}

/// Lazily built pick structures, kept per window across frames.
#[derive(Default)]
pub(crate) struct PickCache {
    grids: HashMap<(u64, u8), (u64, Arc<PointGrid>)>,
}

impl PickCache {
    /// Drops grids of plots that no longer exist.
    pub fn retain(&mut self, alive: impl Fn(u64) -> bool) {
        self.grids.retain(|(uid, _), _| alive(*uid));
    }
}

/// Everything a plot needs to answer a hover query.
pub(crate) struct PickCtx<'a> {
    pub axis: &'a AxisFrame,
    /// Cursor in figure units.
    pub cursor: [f64; 2],
    /// Search radius in figure units (Makie's inspector `range`, 10).
    pub radius: f64,
    pub uid: u64,
    pub data_rev: u64,
    pub theme: &'a Theme,
    pub g: &'a Globals,
    pub cache: &'a mut PickCache,
}

impl PickCtx<'_> {
    /// The point of `pts` (data space) nearest to the cursor within the radius and inside the
    /// axis: `(index, distance, position in figure units)`. Uses a uniform grid in scaled space,
    /// built once per data revision.
    pub fn nearest_point(&mut self, part: u8, pts: &[[f64; 2]]) -> Option<(usize, f64, [f64; 2])> {
        let a = self.axis;
        let (xs, ys) = (a.attrs.xscale, a.attrs.yscale);
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.data_rev, pts.len(), xs, ys).hash(&mut h);
            h.finish()
        };
        let grid = match self.cache.grids.get(&(self.uid, part)) {
            Some((k, g)) if *k == key => g.clone(),
            _ => {
                let g = Arc::new(PointGrid::build(pts, xs, ys));
                self.cache.grids.insert((self.uid, part), (key, g.clone()));
                g
            }
        };
        // Cursor and radius in scaled space.
        let r = a.rect;
        if r.w <= 0.0 || r.h <= 0.0 {
            return None;
        }
        let mut fx = (self.cursor[0] - r.x) / r.w;
        let mut fy = (r.bottom() - self.cursor[1]) / r.h;
        if a.attrs.xreversed {
            fx = 1.0 - fx;
        }
        if a.attrs.yreversed {
            fy = 1.0 - fy;
        }
        let [v0, v1, v2, v3] = a.view;
        let (sx, sy) = (v0 + fx * (v1 - v0), v2 + fy * (v3 - v2));
        let (rx, ry) = (self.radius / r.w * (v1 - v0).abs(), self.radius / r.h * (v3 - v2).abs());
        let mut best: Option<(usize, f64, [f64; 2])> = None;
        grid.query([sx - rx, sx + rx, sy - ry, sy + ry], |i| {
            let p = pts[i];
            let Some(u) = a.to_units(p[0], p[1]) else { return };
            if !r.contains(u) {
                return;
            }
            let d = (u[0] - self.cursor[0]).hypot(u[1] - self.cursor[1]);
            if d <= self.radius && best.is_none_or(|b| d < b.1) {
                best = Some((i, d, u));
            }
        });
        best
    }
}

impl PickCtx<'_> {
    /// The cursor in data space, or `None` outside the axis or the scales' domains.
    pub fn cursor_data(&self) -> Option<[f64; 2]> {
        let a = self.axis;
        let r = a.rect;
        if r.w <= 0.0 || r.h <= 0.0 || !r.contains(self.cursor) {
            return None;
        }
        let mut fx = (self.cursor[0] - r.x) / r.w;
        let mut fy = (r.bottom() - self.cursor[1]) / r.h;
        if a.attrs.xreversed {
            fx = 1.0 - fx;
        }
        if a.attrs.yreversed {
            fy = 1.0 - fy;
        }
        let [v0, v1, v2, v3] = a.view;
        let (x, y) = (a.attrs.xscale.inverse(v0 + fx * (v1 - v0)), a.attrs.yscale.inverse(v2 + fy * (v3 - v2)));
        (x.is_finite() && y.is_finite()).then_some([x, y])
    }

    /// Makie's line inspection: the point of the polyline `pts` (data space, NaN breaks it)
    /// closest to the cursor, measured on screen, within the radius and inside the axis:
    /// `(distance, position in figure units, position in data space)`.
    pub fn nearest_on_polyline<'p>(
        &self,
        pts: impl Iterator<Item = &'p [f64; 2]>,
    ) -> Option<(f64, [f64; 2], [f64; 2])> {
        let a = self.axis;
        let (xs, ys) = (a.attrs.xscale, a.attrs.yscale);
        let [cx, cy] = self.cursor;
        let rad = self.radius;
        let mut best: Option<(f64, [f64; 2], [f64; 2])> = None;
        let mut prev: Option<([f64; 2], [f64; 2])> = None; // (units, scaled)
        for p in pts {
            let s = [xs.forward(p[0]), ys.forward(p[1])];
            let cur = a.to_units(p[0], p[1]).map(|u| (u, s));
            if let (Some((u0, s0)), Some((u1, s1))) = (prev, cur) {
                // Cheap reject: the segment's box is farther than the radius.
                let far = u0[0].max(u1[0]) < cx - rad
                    || u0[0].min(u1[0]) > cx + rad
                    || u0[1].max(u1[1]) < cy - rad
                    || u0[1].min(u1[1]) > cy + rad;
                if !far {
                    let (dx, dy) = (u1[0] - u0[0], u1[1] - u0[1]);
                    let len2 = dx * dx + dy * dy;
                    let t =
                        if len2 > 0.0 { (((cx - u0[0]) * dx + (cy - u0[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
                    let q = [u0[0] + t * dx, u0[1] + t * dy];
                    let d = (q[0] - cx).hypot(q[1] - cy);
                    if d <= rad && a.rect.contains(q) && best.is_none_or(|b| d < b.0) {
                        let data = [xs.inverse(s0[0] + t * (s1[0] - s0[0])), ys.inverse(s0[1] + t * (s1[1] - s0[1]))];
                        best = Some((d, q, data));
                    }
                }
            } else if let Some((u, s)) = cur {
                // An isolated point (start of a run) can be hovered too.
                let d = (u[0] - cx).hypot(u[1] - cy);
                if d <= rad && a.rect.contains(u) && best.is_none_or(|b| d < b.0) {
                    best = Some((d, u, [xs.inverse(s[0]), ys.inverse(s[1])]));
                }
            }
            prev = cur;
        }
        best
    }
}

/// Points bucketed into a uniform grid in scaled space (CSR layout).
pub(crate) struct PointGrid {
    origin: [f64; 2],
    cell: [f64; 2],
    n: [usize; 2],
    start: Vec<u32>,
    idx: Vec<u32>,
}

impl PointGrid {
    pub fn build(pts: &[[f64; 2]], xs: Scale, ys: Scale) -> PointGrid {
        let scaled: Vec<[f64; 2]> = pts.iter().map(|p| [xs.forward(p[0]), ys.forward(p[1])]).collect();
        let mut b = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
        let mut count = 0usize;
        for s in scaled.iter().filter(|s| s[0].is_finite() && s[1].is_finite()) {
            b = [b[0].min(s[0]), b[1].max(s[0]), b[2].min(s[1]), b[3].max(s[1])];
            count += 1;
        }
        if count == 0 {
            return PointGrid { origin: [0.0; 2], cell: [1.0; 2], n: [0, 0], start: vec![0], idx: vec![] };
        }
        // About one point per cell on average.
        let side = ((count as f64).sqrt().ceil() as usize).clamp(1, 2048);
        let size = |lo: f64, hi: f64| if hi > lo { (hi - lo) / side as f64 } else { 1.0 };
        let mut g = PointGrid {
            origin: [b[0], b[2]],
            cell: [size(b[0], b[1]), size(b[2], b[3])],
            n: [side, side],
            start: vec![0; side * side + 1],
            idx: vec![0; count],
        };
        let cells: Vec<Option<usize>> = scaled.iter().map(|s| g.cell_of(*s)).collect();
        for c in cells.iter().flatten() {
            g.start[c + 1] += 1;
        }
        for i in 0..side * side {
            g.start[i + 1] += g.start[i];
        }
        let mut fill = g.start.clone();
        for (i, c) in cells.iter().enumerate() {
            if let Some(c) = c {
                g.idx[fill[*c] as usize] = i as u32;
                fill[*c] += 1;
            }
        }
        g
    }

    fn cell_of(&self, s: [f64; 2]) -> Option<usize> {
        if !(s[0].is_finite() && s[1].is_finite()) {
            return None;
        }
        let cx = self.coord(0, s[0]);
        let cy = self.coord(1, s[1]);
        Some(cy * self.n[0] + cx)
    }

    fn coord(&self, d: usize, v: f64) -> usize {
        let c = ((v - self.origin[d]) / self.cell[d]).floor();
        if c.is_nan() { 0 } else { c.clamp(0.0, (self.n[d].max(1) - 1) as f64) as usize }
    }

    /// Calls `f` with the index of every point in the cells overlapping `[x0, x1, y0, y1]`.
    pub fn query(&self, r: [f64; 4], mut f: impl FnMut(usize)) {
        if self.n[0] == 0 || !r.iter().all(|v| v.is_finite()) {
            return;
        }
        let end = |d: usize, v: f64| v > self.origin[d] + self.cell[d] * self.n[d] as f64;
        if end(0, r[0]) || end(1, r[2]) || r[1] < self.origin[0] || r[3] < self.origin[1] {
            return;
        }
        let (cx0, cx1) = (self.coord(0, r[0]), self.coord(0, r[1]));
        let (cy0, cy1) = (self.coord(1, r[2]), self.coord(1, r[3]));
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                let c = cy * self.n[0] + cx;
                for &i in &self.idx[self.start[c] as usize..self.start[c + 1] as usize] {
                    f(i as usize);
                }
            }
        }
    }
}

/// C's `%.6g`: six significant digits, trailing zeros removed, exponent form for very large or
/// small magnitudes.
pub(crate) fn sig6(v: f64) -> String {
    if !v.is_finite() {
        return format!("{v}");
    }
    if v == 0.0 {
        return "0".into();
    }
    let e = format!("{v:.5e}");
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let trim = |s: &str| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() }
    };
    if !(-4..6).contains(&exp) {
        format!("{}e{}{:02}", trim(mant), if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        trim(&format!("{:.*}", (5 - exp) as usize, v))
    }
}

/// Makie's point readout: `x: …\ny: …` with six significant digits.
pub(crate) fn point_text(x: f64, y: f64) -> String {
    format!("x: {}\ny: {}", sig6(x), sig6(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_significant_digits() {
        assert_eq!(sig6(0.0), "0");
        assert_eq!(sig6(1.0), "1");
        assert_eq!(sig6(-2.5), "-2.5");
        assert_eq!(sig6(std::f64::consts::PI), "3.14159");
        assert_eq!(sig6(123456.7), "123457");
        assert_eq!(sig6(999999.7), "1e+06");
        assert_eq!(sig6(1234567.0), "1.23457e+06");
        assert_eq!(sig6(0.0001234), "0.0001234");
        assert_eq!(sig6(0.00001234), "1.234e-05");
        assert_eq!(sig6(1e9 + 1.0), "1e+09");
        assert_eq!(point_text(1.5, -0.25), "x: 1.5\ny: -0.25");
    }

    #[test]
    fn grid_finds_all_points_in_range() {
        let pts: Vec<[f64; 2]> = (0..1000).map(|i| [(i % 37) as f64, (i / 37) as f64 * 0.5]).collect();
        let g = PointGrid::build(&pts, Scale::Identity, Scale::Identity);
        let q = [9.5, 12.5, 3.0, 4.0];
        let mut found = Vec::new();
        g.query(q, |i| {
            let p = pts[i];
            if p[0] >= q[0] && p[0] <= q[1] && p[1] >= q[2] && p[1] <= q[3] {
                found.push(i);
            }
        });
        found.sort();
        let want: Vec<usize> = (0..1000)
            .filter(|&i| {
                let p = pts[i];
                p[0] >= q[0] && p[0] <= q[1] && p[1] >= q[2] && p[1] <= q[3]
            })
            .collect();
        assert_eq!(found, want);
        assert!(!want.is_empty());
    }
}
