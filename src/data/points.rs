//! Point data: the [`PointData`] input trait and the chunked, append-only point storage behind
//! live line plots.

use super::{Data1D, Scalar};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Data that can be plotted as a sequence of 2D points (Makie's `PointBased` conversion):
///
/// - point lists: `&[[T; 2]]`, `&[(T, T)]`, and their `Vec`/array forms;
/// - any 1D data ([`Data1D`]), taken as y values with x = 1..=n (Makie's `lines(y)`).
///
/// ```
/// use sciplot::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// ax.lines_points(&[[0.0, 1.0], [1.0, 3.0], [2.0, 2.0]]);
/// ax.lines_points(vec![(0, 1), (1, 0)]);
/// ax.lines_points([3.0, 1.0, 2.0]); // y only: x = 1, 2, 3
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not plottable point data",
    label = "expected points like `&[[x, y], ..]` or `&[(x, y), ..]`, or 1D y values",
    note = "for separate x and y vectors use the two-argument form, e.g. `ax.lines(&x, &y)`"
)]
pub trait PointData {
    /// Appends the points (as f64) to `out`.
    fn write_points(self, out: &mut Vec<[f64; 2]>);

    /// Collects into a new vector.
    fn to_points(self) -> Vec<[f64; 2]>
    where
        Self: Sized,
    {
        let mut v = Vec::new();
        self.write_points(&mut v);
        v
    }
}

impl<D: Data1D> PointData for D {
    fn write_points(self, out: &mut Vec<[f64; 2]>) {
        let y = self.to_vec_f64();
        out.extend(y.into_iter().enumerate().map(|(i, y)| [(i + 1) as f64, y]));
    }
}

macro_rules! point_impls {
    ($($t:ty => |$p:ident| $e:expr),* $(,)?) => {$(
        impl<T: Scalar> PointData for $t {
            fn write_points(self, out: &mut Vec<[f64; 2]>) {
                out.extend(self.into_iter().map(|$p| $e));
            }
        }
    )*};
}
point_impls!(
    &[[T; 2]] => |p| [p[0].to_f64(), p[1].to_f64()],
    &Vec<[T; 2]> => |p| [p[0].to_f64(), p[1].to_f64()],
    Vec<[T; 2]> => |p| [p[0].to_f64(), p[1].to_f64()],
    &[(T, T)] => |p| [p.0.to_f64(), p.1.to_f64()],
    &Vec<(T, T)> => |p| [p.0.to_f64(), p.1.to_f64()],
    Vec<(T, T)> => |p| [p.0.to_f64(), p.1.to_f64()],
);

impl<T: Scalar, const N: usize> PointData for [[T; 2]; N] {
    fn write_points(self, out: &mut Vec<[f64; 2]>) {
        out.extend(self.into_iter().map(|p| [p[0].to_f64(), p[1].to_f64()]));
    }
}
impl<T: Scalar, const N: usize> PointData for &[[T; 2]; N] {
    fn write_points(self, out: &mut Vec<[f64; 2]>) {
        out.extend(self.iter().map(|p| [p[0].to_f64(), p[1].to_f64()]));
    }
}
impl<T: Scalar, const N: usize> PointData for [(T, T); N] {
    fn write_points(self, out: &mut Vec<[f64; 2]>) {
        out.extend(self.into_iter().map(|p| [p.0.to_f64(), p.1.to_f64()]));
    }
}
impl<T: Scalar, const N: usize> PointData for &[(T, T); N] {
    fn write_points(self, out: &mut Vec<[f64; 2]>) {
        out.extend(self.iter().map(|p| [p.0.to_f64(), p.1.to_f64()]));
    }
}

/// Points per sealed chunk. Small enough that copying the shared tail on the first `push` after a
/// snapshot is cheap, large enough that snapshots clone few `Arc`s.
pub(crate) const CHUNK: usize = 4096;

/// A sealed, immutable run of exactly [`CHUNK`] points with its cached finite bounds.
#[derive(Clone, Debug)]
struct Chunk {
    pts: Arc<Vec<[f64; 2]>>,
    /// Finite bounds `[x0, x1, y0, y1]` in data space (identity scales).
    bounds: Option<[f64; 4]>,
}

/// Append-only f64 point storage: sealed chunks plus a growing tail. Cloning (figure snapshots)
/// is O(#chunks) `Arc` clones; `push` is O(1) amortized; `set`/`clear` start a new epoch.
#[derive(Clone, Debug)]
pub(crate) struct Points {
    chunks: Vec<Chunk>,
    tail: Arc<Vec<[f64; 2]>>,
    len: usize,
    /// Number of points with a non-finite coordinate (NaN breaks).
    nonfinite: usize,
    /// Process-unique id of this append-only history; changes on every non-append edit.
    epoch: u64,
}

impl Default for Points {
    fn default() -> Self {
        Points::new(Vec::new())
    }
}

impl Points {
    /// Storage holding `pts` (chunked here, so call this off the figure lock).
    pub(crate) fn new(pts: Vec<[f64; 2]>) -> Points {
        let mut p = Points {
            chunks: Vec::new(),
            tail: Arc::new(Vec::new()),
            len: 0,
            nonfinite: 0,
            epoch: crate::figure::next_uid(),
        };
        p.extend_from_slice(&pts);
        p
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Appends one point.
    pub(crate) fn push(&mut self, p: [f64; 2]) {
        let tail = Arc::make_mut(&mut self.tail);
        if tail.capacity() == 0 {
            tail.reserve_exact(CHUNK);
        }
        tail.push(p);
        self.len += 1;
        self.nonfinite += !finite(&p) as usize;
        if tail.len() == CHUNK {
            self.seal();
        }
    }

    /// Appends points.
    pub(crate) fn extend_from_slice(&mut self, mut pts: &[[f64; 2]]) {
        while !pts.is_empty() {
            let tail = Arc::make_mut(&mut self.tail);
            let take = (CHUNK - tail.len()).min(pts.len());
            if tail.capacity() < CHUNK {
                tail.reserve_exact(CHUNK - tail.len());
            }
            tail.extend_from_slice(&pts[..take]);
            self.len += take;
            self.nonfinite += pts[..take].iter().filter(|p| !finite(p)).count();
            pts = &pts[take..];
            if tail.len() == CHUNK {
                self.seal();
            }
        }
    }

    fn seal(&mut self) {
        let full = std::mem::replace(&mut self.tail, Arc::new(Vec::new()));
        let bounds =
            crate::plots::point_bounds(&full, crate::transform::Scale::Identity, crate::transform::Scale::Identity);
        self.chunks.push(Chunk { pts: full, bounds });
    }

    /// Removes every point (a new epoch).
    pub(crate) fn clear(&mut self) {
        *self = Points::new(Vec::new());
    }

    /// Makie's closed-loop test: a single run (no NaN) of 3+ segments whose last point equals the
    /// first (within `sqrt(eps)`, like `isapprox`).
    pub(crate) fn is_closed(&self) -> bool {
        let first = self.chunks.first().map_or(self.tail.first(), |c| c.pts.first());
        let last = self.tail.last().or_else(|| self.chunks.last().and_then(|c| c.pts.last()));
        let close = |a: f64, b: f64| (a - b).abs() <= f64::EPSILON.sqrt() * a.abs().max(b.abs());
        match (first, last) {
            (Some(a), Some(b)) if self.len >= 4 && self.nonfinite == 0 => close(a[0], b[0]) && close(a[1], b[1]),
            _ => false,
        }
    }

    /// Contiguous slices in order.
    pub(crate) fn slices(&self) -> impl Iterator<Item = &[[f64; 2]]> {
        self.chunks.iter().map(|c| c.pts.as_slice()).chain(std::iter::once(self.tail.as_slice()))
    }

    /// Points from index `from` on.
    pub(crate) fn iter_from(&self, from: usize) -> impl Iterator<Item = &[f64; 2]> {
        let first = (from / CHUNK).min(self.chunks.len());
        let skip = from - first * CHUNK;
        self.slices().skip(first).flatten().skip(skip)
    }

    /// All points.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &[f64; 2]> {
        self.slices().flatten()
    }

    /// Finite bounds in scaled space `[x0, x1, y0, y1]`; O(#chunks + CHUNK) for identity scales.
    pub(crate) fn bounds(&self, xs: crate::transform::Scale, ys: crate::transform::Scale) -> Option<[f64; 4]> {
        use crate::transform::Scale::Identity;
        let union = |a: Option<[f64; 4]>, b: Option<[f64; 4]>| match (a, b) {
            (Some(a), Some(b)) => Some([a[0].min(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].max(b[3])]),
            (x, None) | (None, x) => x,
        };
        if xs == Identity && ys == Identity {
            let sealed = self.chunks.iter().fold(None, |acc, c| union(acc, c.bounds));
            union(sealed, crate::plots::point_bounds(&self.tail, xs, ys))
        } else {
            self.slices().fold(None, |acc, s| union(acc, crate::plots::point_bounds(s, xs, ys)))
        }
    }
}

fn finite(p: &[f64; 2]) -> bool {
    p[0].is_finite() && p[1].is_finite()
}

/// Encodes an append-only buffer revision: a generation (unique per conversion history) in the
/// high 32 bits and the element count in the low 32. Buffers of one generation are prefixes of each
/// other, so a GPU cache holding generation `g` with `n0` elements only needs the tail.
pub(crate) fn append_rev(generation: u32, len: usize) -> u64 {
    ((generation as u64) << 32) | (len as u64 & 0xFFFF_FFFF)
}

/// Inverse of [`append_rev`]: `(generation, len)`.
pub(crate) fn split_append_rev(rev: u64) -> (u32, usize) {
    ((rev >> 32) as u32, (rev & 0xFFFF_FFFF) as usize)
}

fn next_generation() -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Append-aware memo of one plot part's local f32 conversion (per render context).
///
/// While the conversion inputs (`base`: data epoch, rebase, scales) are unchanged, new points are
/// converted and appended in place. A small pool of buffers lets the previous frame's draw list keep
/// its `Arc` while this frame appends to another one, so live appends stay O(new points).
#[derive(Default)]
pub(crate) struct LocalCache {
    base: u64,
    generation: u32,
    pool: Vec<Arc<Vec<[f32; 2]>>>,
}

impl LocalCache {
    /// The converted points and their [`append_rev`] revision.
    pub(crate) fn update(
        &mut self,
        base: u64,
        pts: &Points,
        f: impl Fn(&[f64; 2]) -> [f32; 2],
    ) -> (Arc<Vec<[f32; 2]>>, u64) {
        let n = pts.len();
        let usable = self.base == base && !self.pool.is_empty() && self.pool.iter().all(|b| b.len() <= n);
        if !usable {
            self.base = base;
            self.generation = next_generation();
            self.pool.clear();
            self.pool.push(Arc::new(pts.iter().map(&f).collect()));
            return (self.pool[0].clone(), append_rev(self.generation, n));
        }
        // Longest buffer nobody else holds; otherwise a copy of the longest one.
        let free =
            (0..self.pool.len()).filter(|&i| Arc::strong_count(&self.pool[i]) == 1).max_by_key(|&i| self.pool[i].len());
        let i = match free {
            Some(i) => i,
            None => {
                let longest = self.pool.iter().max_by_key(|b| b.len()).cloned().unwrap_or_default();
                let mut v = Vec::with_capacity(n + n / 2);
                v.extend_from_slice(&longest);
                if self.pool.len() >= 3 {
                    self.pool.remove(0);
                }
                self.pool.push(Arc::new(v));
                self.pool.len() - 1
            }
        };
        let buf = &mut self.pool[i];
        if let Some(v) = Arc::get_mut(buf) {
            let have = v.len();
            v.extend(pts.iter_from(have).map(&f));
        }
        (buf.clone(), append_rev(self.generation, n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_inputs() {
        assert_eq!((&[[1.0, 2.0], [3.0, 4.0]]).to_points(), [[1.0, 2.0], [3.0, 4.0]]);
        assert_eq!(vec![(1, 2), (3, 4)].to_points(), [[1.0, 2.0], [3.0, 4.0]]);
        assert_eq!((&vec![[1f32, 2.0]]).to_points(), [[1.0, 2.0]]);
        assert_eq!([5.0, 7.0].to_points(), [[1.0, 5.0], [2.0, 7.0]]);
        assert_eq!((0..3).to_points(), [[1.0, 0.0], [2.0, 1.0], [3.0, 2.0]]);
        let v = [3.0f64, 4.0];
        assert_eq!(v.iter().map(|y| y * 2.0).to_points(), [[1.0, 6.0], [2.0, 8.0]]);
    }

    #[test]
    fn chunked_storage() {
        let n = 3 * CHUNK + 17;
        let all: Vec<[f64; 2]> = (0..n).map(|i| [i as f64, -(i as f64)]).collect();
        let mut p = Points::new(all[..CHUNK + 5].to_vec());
        let snapshot = p.clone();
        for q in &all[CHUNK + 5..2 * CHUNK] {
            p.push(*q);
        }
        p.extend_from_slice(&all[2 * CHUNK..]);
        assert_eq!(p.len(), n);
        assert_eq!(p.iter().copied().collect::<Vec<_>>(), all);
        assert_eq!(p.iter_from(CHUNK + 3).next(), Some(&all[CHUNK + 3]));
        assert_eq!(p.iter_from(2 * CHUNK).count(), n - 2 * CHUNK);
        assert_eq!(p.iter_from(n).count(), 0);
        // The snapshot is unaffected by later appends.
        assert_eq!(snapshot.len(), CHUNK + 5);
        assert_eq!(snapshot.iter().count(), CHUNK + 5);
        assert_eq!(snapshot.epoch(), p.epoch());
        assert_eq!(
            p.bounds(Default::default(), Default::default()),
            Some([0.0, (n - 1) as f64, -((n - 1) as f64), 0.0])
        );
        assert!(!p.is_closed());
        let e = p.epoch();
        p.clear();
        assert!(p.is_empty() && p.epoch() != e);
    }

    #[test]
    fn closed_loops() {
        let sq = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]];
        assert!(Points::new(sq.clone()).is_closed());
        assert!(!Points::new(sq[..3].to_vec()).is_closed());
        let mut p = Points::new(sq);
        p.push([f64::NAN, 0.0]);
        p.push([0.0, 0.0]);
        assert!(!p.is_closed());
    }

    #[test]
    fn bounds_skip_non_finite_and_log_domain() {
        let p = Points::new(vec![[f64::NAN, 1.0], [-1.0, 5.0], [10.0, 100.0]]);
        let lg = crate::transform::Scale::Log10;
        assert_eq!(p.bounds(Default::default(), Default::default()), Some([-1.0, 10.0, 5.0, 100.0]));
        assert_eq!(p.bounds(lg, lg), Some([1.0, 1.0, 2.0, 2.0]));
    }

    #[test]
    fn append_conversion_reuses_prefix() {
        let f = |p: &[f64; 2]| [p[0] as f32, p[1] as f32];
        let mut p = Points::new((0..10).map(|i| [i as f64, 0.0]).collect());
        let mut c = LocalCache::default();
        let (a, ra) = c.update(1, &p, f);
        assert_eq!(a.len(), 10);
        // A frame still holds `a`: the next append goes into a second buffer.
        p.push([10.0, 0.0]);
        let (b, rb) = c.update(1, &p, f);
        assert_eq!(b.len(), 11);
        assert_eq!(split_append_rev(ra).0, split_append_rev(rb).0);
        assert_eq!(split_append_rev(rb).1, 11);
        drop(a);
        p.push([11.0, 0.0]);
        let (d, rd) = c.update(1, &p, f);
        assert_eq!(d.as_slice(), &(0..12).map(|i| [i as f32, 0.0]).collect::<Vec<_>>()[..]);
        assert_eq!(split_append_rev(rd), (split_append_rev(ra).0, 12));
        // A new base (e.g. a rebase) starts a new generation.
        let (_, re) = c.update(2, &p, f);
        assert_ne!(split_append_rev(re).0, split_append_rev(rd).0);
    }
}
