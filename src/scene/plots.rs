//! Lowering plots to draw-list primitives: cycling, then each plot type's `PlotImpl::emit`.

use super::drawlist::{Buf, BufKey, Emitter, Prim, PrimColor, Rect, Space};
use super::{AxisFrame, SceneCache};
use crate::color::Color;
use crate::figure::FigState;
use crate::plots::ColorSpec;
use crate::theme::{Globals, Theme};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Everything a plot needs to lower itself.
pub(crate) struct PlotCtx<'a> {
    pub em: &'a mut Emitter,
    pub axis: &'a AxisFrame,
    pub g: &'a Globals,
    pub theme: &'a Theme,
    pub cache: &'a mut SceneCache,
    /// The plot's unique id (cache key for its GPU buffers).
    pub uid: u64,
    /// The plot's data revision.
    pub data_rev: u64,
    /// This plot's index in its cycle group (0-based).
    pub cycle: usize,
    /// Draw order within the axis.
    pub z: f32,
}

impl PlotCtx<'_> {
    /// Emits a primitive in this axis' data space, clipped to the axis.
    pub fn push_data(&mut self, prim: Prim) {
        self.em.push(self.z, Some(self.axis.rect), Space::Data(self.axis.slot), prim);
    }

    /// Emits a primitive in figure units, clipped to the axis.
    #[allow(dead_code)]
    pub fn push_figure(&mut self, prim: Prim) {
        self.em.push(self.z, Some(self.axis.rect), Space::Figure, prim);
    }

    /// The axis rectangle in figure units.
    #[allow(dead_code)]
    pub fn rect(&self) -> Rect {
        self.axis.rect
    }

    /// Converts data points to local f32 coordinates (memoized by `part` + data revision + rebase),
    /// returning a GPU-cacheable buffer.
    pub fn local_points(&mut self, part: u8, pts: &[[f64; 2]]) -> Buf<[f32; 2]> {
        let key = self.conv_key(part);
        let a = self.axis;
        let data = self.cache.convert(self.uid, part, key, || to_local(pts, a));
        Buf { key: Some(BufKey { uid: self.uid, part, rev: key }), data }
    }

    /// Like [`local_points`](Self::local_points) for append-only point storage: after a `push`,
    /// only the new points are converted, and the buffer's revision
    /// ([`append_rev`](crate::data::points::append_rev)) lets the GPU upload only the tail.
    pub fn local_points_append(&mut self, part: u8, pts: &crate::data::points::Points) -> Buf<[f32; 2]> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (pts.epoch(), part, self.axis.rebase.epoch, self.axis.attrs.xscale, self.axis.attrs.yscale).hash(&mut h);
        let a = self.axis;
        let (xs, ys) = (a.attrs.xscale, a.attrs.yscale);
        let conv = |p: &[f64; 2]| {
            let (sx, sy) = (xs.forward(p[0]), ys.forward(p[1]));
            if sx.is_finite() && sy.is_finite() { a.rebase.to_local(sx, sy) } else { [f32::NAN, f32::NAN] }
        };
        let (data, rev) = self.cache.append.entry((self.uid, part)).or_default().update(h.finish(), pts, conv);
        Buf { key: Some(BufKey { uid: self.uid, part, rev }), data }
    }

    /// A cache key combining the data revision, the axis rebase and the scales.
    pub fn conv_key(&self, part: u8) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.data_rev, part, self.axis.rebase.epoch, self.axis.attrs.xscale, self.axis.attrs.yscale).hash(&mut h);
        h.finish()
    }

    /// A GPU buffer that changes only with the plot's data.
    pub fn data_buf<T>(&self, part: u8, data: Arc<Vec<T>>) -> Buf<T> {
        Buf { key: Some(BufKey { uid: self.uid, part, rev: self.data_rev }), data }
    }

    /// Resolves a color spec against the line/marker palette (`patch = false`) or the fill
    /// palette (`patch = true`). `Values` and `PerPoint` specs return `None`.
    pub fn solid_color(&self, spec: &ColorSpec, patch: bool) -> Option<Color> {
        let pal = if patch { &self.g.patchpalette } else { &self.g.palette };
        resolve_color(spec, self.cycle, pal)
    }

    /// Per-element premultiplied colors for `PerPoint` specs, with `alpha` applied.
    pub fn per_point(&self, part: u8, colors: &[Color], alpha: f32) -> PrimColor {
        let data = Arc::new(colors.iter().map(|c| c.with_alpha(c.a * alpha).to_premul_u32()).collect());
        PrimColor::PerElement(self.data_buf(part, data))
    }
}

/// Scaled + rebased local coordinates for data points (NaN for points outside the scale domain).
pub(crate) fn to_local(pts: &[[f64; 2]], a: &AxisFrame) -> Vec<[f32; 2]> {
    let (xs, ys) = (a.attrs.xscale, a.attrs.yscale);
    pts.iter()
        .map(|p| {
            let (sx, sy) = (xs.forward(p[0]), ys.forward(p[1]));
            if sx.is_finite() && sy.is_finite() { a.rebase.to_local(sx, sy) } else { [f32::NAN, f32::NAN] }
        })
        .collect()
}

/// Resolves a cycled color.
pub(crate) fn resolve_color(spec: &ColorSpec, cycle: usize, palette: &[Color]) -> Option<Color> {
    let n = palette.len().max(1);
    match spec {
        ColorSpec::Auto => Some(palette[cycle % n]),
        ColorSpec::Solid(c) => Some(*c),
        ColorSpec::Cycled(i) => Some(palette[((*i).max(1) - 1) % n]),
        _ => None,
    }
}

pub(crate) fn emit_plots(em: &mut Emitter, st: &FigState, a: &AxisFrame, g: &Globals, cache: &mut SceneCache) {
    let Some(ax) = st.block(a.id).and_then(|b| b.as_axis()) else {
        return;
    };
    let mut counters: HashMap<&'static str, usize> = HashMap::new();
    for pid in &ax.plots {
        let Some(p) = st.plot(*pid) else { continue };
        let imp = p.kind.imp();
        // Only plots whose cycled attribute is automatic advance the counter (Makie).
        let cycle = if imp.color_is_auto(&st.theme) {
            let c = counters.entry(imp.cycle_group()).or_insert(0);
            *c += 1;
            *c - 1
        } else {
            0
        };
        if !p.common.visible {
            continue;
        }
        let mut ctx = PlotCtx {
            em: &mut *em,
            axis: a,
            g,
            theme: &st.theme,
            cache: &mut *cache,
            uid: p.uid,
            data_rev: p.data_rev,
            cycle,
            z: p.common.z,
        };
        imp.emit(&mut ctx);
    }
}
