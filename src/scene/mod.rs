//! Scene building: figure snapshot -> resolved attributes -> limits -> ticks -> layout -> DrawList.
//!
//! Runs on the rendering thread, outside the figure lock. A `SceneCache` per render context keeps
//! converted f32 buffers and per-axis rebases so unchanged data is never reconverted or reuploaded.

pub(crate) mod axis;
pub(crate) mod drawlist;
mod plots;

pub(crate) use plots::PlotCtx;

use crate::blocks::Block;
use crate::blocks::axis::AxisResolved;
use crate::figure::{BlockId, FigState};
use crate::layout::{LayoutItem, Protrusion};
use crate::theme::Globals;
use crate::transform::Rebase;
use drawlist::{AxisXform, DrawList, Emitter, Rect};
use std::collections::HashMap;
use std::sync::Arc;

/// Per-render-context memo of converted buffers and axis rebases.
#[derive(Default)]
pub(crate) struct SceneCache {
    pub rebases: HashMap<BlockId, Rebase>,
    epoch: u64,
    /// (plot uid, part) -> (conversion key, converted data)
    pub(crate) conv: HashMap<(u64, u8), (u64, Arc<Vec<[f32; 2]>>)>,
}

impl SceneCache {
    pub fn new() -> SceneCache {
        SceneCache::default()
    }

    /// A rebase adequate for `view` (scaled space), reusing the previous one when possible.
    fn rebase_for(&mut self, axis: BlockId, view: [f64; 4]) -> Rebase {
        if let Some(r) = self.rebases.get(&axis) {
            if r.adequate(view) {
                return *r;
            }
        }
        self.epoch += 1;
        let r = Rebase::for_view(view, self.epoch);
        self.rebases.insert(axis, r);
        r
    }

    /// Converts f64 data points to local f32 through `f`, memoized on `key`.
    pub(crate) fn convert(
        &mut self,
        uid: u64,
        part: u8,
        key: u64,
        f: impl FnOnce() -> Vec<[f32; 2]>,
    ) -> Arc<Vec<[f32; 2]>> {
        if let Some((k, d)) = self.conv.get(&(uid, part)) {
            if *k == key {
                return d.clone();
            }
        }
        let d = Arc::new(f());
        self.conv.insert((uid, part), (key, d.clone()));
        d
    }
}

/// One axis during a build.
pub(crate) struct AxisFrame {
    pub id: BlockId,
    pub slot: u16,
    pub attrs: AxisResolved,
    /// Visible limits in data space `[x0, x1, y0, y1]` with `x0 < x1`, `y0 < y1`.
    pub limits: [f64; 4],
    /// Visible limits in scaled space (log etc. applied).
    pub view: [f64; 4],
    pub rebase: Rebase,
    pub xticks: crate::ticks::Ticks,
    pub yticks: crate::ticks::Ticks,
    pub rect: Rect,
}

/// Builds a frame. `size` overrides the figure size (window size).
pub(crate) fn build(st: &FigState, size: Option<[f64; 2]>, cache: &mut SceneCache) -> (DrawList, Vec<AxisFrame>) {
    let g: Globals = st.theme.globals();
    let size = size.unwrap_or(g.size);

    // 1. Axes: resolve attributes and limits.
    let mut axes: Vec<AxisFrame> = Vec::new();
    let mut items: Vec<LayoutItem> = Vec::new();
    let axis_ids: Vec<BlockId> =
        st.iter_blocks().filter(|(_, s)| matches!(s.block, Block::Axis(_))).map(|(id, _)| id).collect();
    let limits = axis::compute_limits(st, &axis_ids, &g);
    for (slot, (id, bslot)) in st.iter_blocks().filter(|(_, s)| matches!(s.block, Block::Axis(_))).enumerate() {
        let Block::Axis(ax) = &bslot.block;
        let attrs = ax.attrs.resolve(&st.theme.axis, &g);
        let lim = limits[slot];
        let view = [
            attrs.xscale.forward(lim[0]),
            attrs.xscale.forward(lim[1]),
            attrs.yscale.forward(lim[2]),
            attrs.yscale.forward(lim[3]),
        ];
        let rebase = cache.rebase_for(id, view);
        let xticks = crate::ticks::major_ticks(lim[0], lim[1], attrs.xscale);
        let yticks = crate::ticks::major_ticks(lim[2], lim[3], attrs.yscale);
        let protrusion = axis::protrusion(&attrs, &xticks, &yticks);
        items.push(LayoutItem {
            rows: bslot.place.rows,
            cols: bslot.place.cols,
            side: bslot.place.side,
            protrusion,
            width: attrs.width.into(),
            height: attrs.height.into(),
            tellwidth: true,
            tellheight: true,
            round: true,
            ..Default::default()
        });
        axes.push(AxisFrame {
            id,
            slot: slot as u16,
            attrs,
            limits: lim,
            view,
            rebase,
            xticks,
            yticks,
            rect: Rect::default(),
        });
    }

    // 2. Layout.
    let rects = crate::layout::solve(&items, &st.grid, size, g.figure_padding, g.colgap, g.rowgap);
    for (a, r) in axes.iter_mut().zip(rects) {
        a.rect = r;
    }

    // 3. Emit.
    let mut em = Emitter::new();
    let mut xforms = Vec::with_capacity(axes.len());
    for a in &axes {
        let lv = a.rebase.local_view(a.view);
        let view = [
            if a.attrs.xreversed { lv[1] } else { lv[0] },
            if a.attrs.xreversed { lv[0] } else { lv[1] },
            if a.attrs.yreversed { lv[3] } else { lv[2] },
            if a.attrs.yreversed { lv[2] } else { lv[3] },
        ];
        xforms.push(AxisXform { rect: a.rect, view });
    }
    for a in &axes {
        axis::emit_decorations(&mut em, a, &xforms[a.slot as usize]);
        plots::emit_plots(&mut em, st, a, &g, cache);
    }

    let mut dl = DrawList { size, background: g.backgroundcolor, axes: xforms, items: em.items };
    dl.sort();
    (dl, axes)
}

#[allow(dead_code)]
pub(crate) fn empty_protrusion() -> Protrusion {
    Protrusion::default()
}
