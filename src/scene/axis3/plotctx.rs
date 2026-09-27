//! What 3D plot types see while they are lowered, and their append-only point storage.

use super::Axis3Frame;
use crate::color::Color;
use crate::data::points::append_rev;
use crate::plots::{ColorSpec, PlotKind};
use crate::scene::SceneCache;
use crate::scene::drawlist::{Buf, BufKey, Emitter, Prim, Space, View3d};
use crate::theme::{Globals, Theme};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// What every 3D plot type implements (besides `PlotImpl`, which gives cycling and colormaps).
pub(crate) trait Plot3dImpl {
    /// Finite data bounds `[x0, x1, y0, y1, z0, z1]`.
    fn bounds(&self) -> Option<[f64; 6]>;
    /// Lowers the plot to 3D primitives.
    fn emit3(&self, ctx: &mut Plot3dCtx<'_>);
    /// Meshes are drawn before lines and markers (they write depth).
    fn is_mesh(&self) -> bool {
        false
    }
}

/// The 3D implementation of a plot, if it is a 3D plot type.
pub(crate) fn plot3d(kind: &PlotKind) -> Option<&dyn Plot3dImpl> {
    match kind {
        PlotKind::Lines3d(s) => Some(s),
        PlotKind::Scatter3d(s) => Some(s),
        PlotKind::Surface(s) => Some(s),
        _ => None,
    }
}

/// Everything a 3D plot needs to lower itself.
pub(crate) struct Plot3dCtx<'a> {
    pub em: &'a mut Emitter,
    pub frame: &'a Axis3Frame,
    pub view: Arc<View3d>,
    pub g: &'a Globals,
    pub theme: &'a Theme,
    pub cache: &'a mut SceneCache,
    pub uid: u64,
    pub data_rev: u64,
    /// This plot's index in its cycle group.
    pub cycle: usize,
    pub z: f32,
}

impl Plot3dCtx<'_> {
    /// Emits a 3D primitive (clipped to the scene area).
    pub fn push(&mut self, prim: Prim) {
        self.em.push(self.z, Some(self.frame.area), Space::Figure, prim);
    }

    /// A cache key combining the data revision, `part` and the axis' rebase.
    pub fn conv_key(&self, part: u8) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.data_rev, part, self.frame.rebase.epoch).hash(&mut h);
        h.finish()
    }

    /// Local f32 coordinates of `pts` (memoized on the data revision and rebase), GPU-cacheable.
    pub fn local_points(&mut self, part: u8, pts: &[[f64; 3]]) -> Buf<[f32; 3]> {
        let key = self.conv_key(part);
        let rb = self.frame.rebase;
        let data = self.cache.memo(self.uid, part, key, || pts.iter().map(|p| rb.to_local(*p)).collect());
        Buf { key: Some(BufKey { uid: self.uid, part, rev: key }), data }
    }

    /// Like [`local_points`](Self::local_points) for append-only storage: after a `push` only the
    /// new points are converted, and the buffer's [`append_rev`] revision lets the GPU upload only
    /// the tail.
    pub fn local_points_append(&mut self, part: u8, pts: &Points3) -> Buf<[f32; 3]> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (pts.epoch, part, self.frame.rebase.epoch).hash(&mut h);
        let rb = self.frame.rebase;
        let (data, rev) =
            self.cache.axis3.append.entry((self.uid, part)).or_default().update(h.finish(), pts, |p| rb.to_local(*p));
        Buf { key: Some(BufKey { uid: self.uid, part, rev }), data }
    }

    /// A GPU buffer that changes only with the plot's data (and `part`).
    pub fn data_buf<T>(&self, part: u8, data: Arc<Vec<T>>) -> Buf<T> {
        Buf { key: Some(BufKey { uid: self.uid, part, rev: self.data_rev }), data }
    }

    /// Resolves a color spec against the palette (`Values` and `PerPoint` give `None`).
    pub fn solid_color(&self, spec: &ColorSpec) -> Option<Color> {
        crate::scene::resolve_color(spec, self.cycle, &self.g.palette)
    }
}

/// Points per sealed chunk of [`Points3`].
const CHUNK: usize = 4096;

/// Append-only 3D point storage (the 3D sibling of `data::points::Points`): sealed chunks of
/// [`CHUNK`] points with cached bounds plus a tail, so snapshots are cheap and `push` is O(1)
/// amortized.
#[derive(Clone, Debug)]
pub(crate) struct Points3 {
    chunks: Vec<(Arc<Vec<[f64; 3]>>, Option<[f64; 6]>)>,
    tail: Arc<Vec<[f64; 3]>>,
    len: usize,
    /// Changes on every edit that is not an append.
    pub epoch: u64,
}

impl Default for Points3 {
    fn default() -> Self {
        Points3::new(Vec::new())
    }
}

fn bounds3(pts: &[[f64; 3]]) -> Option<[f64; 6]> {
    let mut b = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
    for p in pts {
        if p.iter().all(|v| v.is_finite()) {
            for i in 0..3 {
                b[2 * i] = b[2 * i].min(p[i]);
                b[2 * i + 1] = b[2 * i + 1].max(p[i]);
            }
        }
    }
    (b[0] <= b[1]).then_some(b)
}

/// Union of two optional bounds.
pub(crate) fn union3(a: Option<[f64; 6]>, b: Option<[f64; 6]>) -> Option<[f64; 6]> {
    match (a, b) {
        (Some(a), Some(b)) => Some(std::array::from_fn(|i| if i % 2 == 0 { a[i].min(b[i]) } else { a[i].max(b[i]) })),
        (x, None) | (None, x) => x,
    }
}

impl Points3 {
    pub(crate) fn new(pts: Vec<[f64; 3]>) -> Points3 {
        let mut p =
            Points3 { chunks: Vec::new(), tail: Arc::new(Vec::new()), len: 0, epoch: crate::figure::next_uid() };
        p.extend_from_slice(&pts);
        p
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn push(&mut self, p: [f64; 3]) {
        self.extend_from_slice(&[p]);
    }

    pub(crate) fn extend_from_slice(&mut self, mut pts: &[[f64; 3]]) {
        while !pts.is_empty() {
            let tail = Arc::make_mut(&mut self.tail);
            let take = (CHUNK - tail.len()).min(pts.len());
            tail.extend_from_slice(&pts[..take]);
            self.len += take;
            pts = &pts[take..];
            if tail.len() == CHUNK {
                let full = std::mem::replace(&mut self.tail, Arc::new(Vec::new()));
                let b = bounds3(&full);
                self.chunks.push((full, b));
            }
        }
    }

    fn slices(&self) -> impl Iterator<Item = &[[f64; 3]]> {
        self.chunks.iter().map(|c| c.0.as_slice()).chain(std::iter::once(self.tail.as_slice()))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &[f64; 3]> {
        self.slices().flatten()
    }

    pub(crate) fn iter_from(&self, from: usize) -> impl Iterator<Item = &[f64; 3]> {
        let first = (from / CHUNK).min(self.chunks.len());
        let skip = from - first * CHUNK;
        self.slices().skip(first).flatten().skip(skip)
    }

    /// Finite bounds `[x0, x1, y0, y1, z0, z1]` in O(#chunks + CHUNK).
    pub(crate) fn bounds(&self) -> Option<[f64; 6]> {
        let sealed = self.chunks.iter().fold(None, |acc, c| union3(acc, c.1));
        union3(sealed, bounds3(&self.tail))
    }
}

fn next_generation() -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Append-aware memo of one plot part's local conversion (per render context).
#[derive(Default)]
pub(crate) struct LocalCache3 {
    base: u64,
    generation: u32,
    data: Arc<Vec<[f32; 3]>>,
}

impl LocalCache3 {
    /// The converted points and their [`append_rev`] revision.
    pub(crate) fn update(
        &mut self,
        base: u64,
        pts: &Points3,
        f: impl Fn(&[f64; 3]) -> [f32; 3],
    ) -> (Arc<Vec<[f32; 3]>>, u64) {
        let n = pts.len();
        let have = self.data.len();
        if self.base != base || self.generation == 0 || have > n {
            self.base = base;
            self.generation = next_generation();
            self.data = Arc::new(pts.iter().map(&f).collect());
        } else if have < n {
            // The previous frame's draw list may still hold the buffer: then extend a copy.
            let v = Arc::make_mut(&mut self.data);
            v.extend(pts.iter_from(have).map(&f));
        }
        (self.data.clone(), append_rev(self.generation, n))
    }
}
