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
    /// (plot uid, part) -> (key, any derived data), for `memo`.
    memos: HashMap<(u64, u8), (u64, Arc<dyn std::any::Any + Send + Sync>)>,
    /// (plot uid, part) -> append-aware conversion of live, append-only point data
    pub(crate) append: HashMap<(u64, u8), crate::data::points::LocalCache>,
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

    /// Computes derived per-plot data once per `key` (e.g. histogram bins, band triangles).
    pub(crate) fn memo<T: Send + Sync + 'static>(
        &mut self,
        uid: u64,
        part: u8,
        key: u64,
        f: impl FnOnce() -> Vec<T>,
    ) -> Arc<Vec<T>> {
        if let Some((k, d)) = self.memos.get(&(uid, part)) {
            if *k == key {
                if let Ok(v) = d.clone().downcast::<Vec<T>>() {
                    return v;
                }
            }
        }
        let d = Arc::new(f());
        self.memos.insert((uid, part), (key, d.clone()));
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

impl AxisFrame {
    /// Data -> figure units (y down), or `None` outside the scale domain.
    pub(crate) fn to_units(&self, x: f64, y: f64) -> Option<[f64; 2]> {
        let (sx, sy) = (self.attrs.xscale.forward(x), self.attrs.yscale.forward(y));
        if !(sx.is_finite() && sy.is_finite()) {
            return None;
        }
        let mut fx = (sx - self.view[0]) / (self.view[1] - self.view[0]);
        let mut fy = (sy - self.view[2]) / (self.view[3] - self.view[2]);
        if self.attrs.xreversed {
            fx = 1.0 - fx;
        }
        if self.attrs.yreversed {
            fy = 1.0 - fy;
        }
        Some([self.rect.x + fx * self.rect.w, self.rect.bottom() - fy * self.rect.h])
    }
}

/// Builds a frame. `size` overrides the figure size (window size).
pub(crate) fn build(st: &FigState, size: Option<[f64; 2]>, cache: &mut SceneCache) -> (DrawList, Vec<AxisFrame>) {
    let g: Globals = st.theme.globals();
    let size = size.unwrap_or(g.size);

    // 1. Layout requests, in block order: axes (after limits and ticks) and other blocks.
    enum Owner {
        Axis(usize),
        Block(BlockId),
    }
    let mut axes: Vec<AxisFrame> = Vec::new();
    let mut items: Vec<LayoutItem> = Vec::new();
    let mut owners: Vec<Owner> = Vec::new();
    let mut inside: Vec<(BlockId, BlockId)> = Vec::new();
    let axis_ids: Vec<BlockId> =
        st.iter_blocks().filter(|(_, s)| matches!(s.block, Block::Axis(_))).map(|(id, _)| id).collect();
    let limits = axis::compute_limits(st, &axis_ids, &g);
    for (id, bslot) in st.iter_blocks() {
        let ax = match &bslot.block {
            Block::Axis(ax) => ax,
            other => {
                let Some(imp) = other.imp() else { continue };
                if let Some(target) = imp.inside_axis() {
                    inside.push((id, target));
                    continue;
                }
                let ctx = crate::blocks::BlockCtx { st, g: &g, axes: &[], id };
                let bl = imp.layout(&ctx);
                items.push(LayoutItem {
                    rows: bslot.place.rows,
                    cols: bslot.place.cols,
                    side: bslot.place.side,
                    protrusion: bl.protrusion,
                    width: bl.width,
                    height: bl.height,
                    autosize: bl.autosize,
                    tellwidth: bl.tellwidth,
                    tellheight: bl.tellheight,
                    halign: bl.halign,
                    valign: bl.valign,
                    alignmode: bl.alignmode,
                    round: false,
                });
                owners.push(Owner::Block(id));
                continue;
            }
        };
        let slot = axes.len();
        let attrs = ax.attrs.resolve(&st.theme.axis, &g);
        let lim = limits[slot];
        let view = [
            attrs.xscale.forward(lim[0]),
            attrs.xscale.forward(lim[1]),
            attrs.yscale.forward(lim[2]),
            attrs.yscale.forward(lim[3]),
        ];
        let rebase = cache.rebase_for(id, view);
        let mut xticks = crate::ticks::resolve_ticks(&attrs.xticks, &attrs.xtickformat, lim[0], lim[1], attrs.xscale);
        let mut yticks = crate::ticks::resolve_ticks(&attrs.yticks, &attrs.ytickformat, lim[2], lim[3], attrs.yscale);
        // Categorical plots (e.g. barplot with names) label their axis with the categories.
        for pid in &ax.plots {
            if let Some((on_x, cats)) = st.plot(*pid).and_then(|p| p.kind.imp().categories()) {
                let t = crate::ticks::Ticks {
                    values: (1..=cats.len()).map(|i| i as f64).collect(),
                    labels: cats.iter().map(|c| crate::text::RichText::from(c.as_str())).collect(),
                };
                if on_x && matches!(attrs.xticks, crate::ticks::TickSpec::Automatic) {
                    xticks = t;
                } else if !on_x && matches!(attrs.yticks, crate::ticks::TickSpec::Automatic) {
                    yticks = t;
                }
            }
        }
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
        owners.push(Owner::Axis(slot));
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
    let mut block_rects: Vec<(BlockId, Rect)> = Vec::new();
    for (o, r) in owners.iter().zip(rects) {
        match o {
            Owner::Axis(s) => axes[*s].rect = r,
            Owner::Block(id) => block_rects.push((*id, r)),
        }
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
    let axis_rect = |id: BlockId| axes.iter().find(|a| a.id == id).map(|a| a.rect);
    for (id, r) in
        block_rects.into_iter().chain(inside.into_iter().filter_map(|(id, ax)| axis_rect(ax).map(|r| (id, r))))
    {
        if let Some(imp) = st.block(id).and_then(|b| b.imp()) {
            imp.emit(&crate::blocks::BlockCtx { st, g: &g, axes: &axes, id }, &mut em, r);
        }
    }

    let mut dl = DrawList { size, background: g.backgroundcolor, axes: xforms, items: em.items };
    dl.sort();
    (dl, axes)
}

#[allow(dead_code)]
pub(crate) fn empty_protrusion() -> Protrusion {
    Protrusion::default()
}
