//! Scene building: figure snapshot -> resolved attributes -> limits -> ticks -> layout -> DrawList.
//!
//! Runs on the rendering thread, outside the figure lock. A `SceneCache` per render context keeps
//! converted f32 buffers and per-axis rebases so unchanged data is never reconverted or reuploaded.

pub(crate) mod axis;
pub(crate) mod drawlist;
mod plots;

pub(crate) use plots::PlotCtx;
pub(crate) use plots::{cycle_indices, resolve_color};

use crate::blocks::Block;
use crate::blocks::axis::AxisResolved;
use crate::figure::{BlockId, FigState};
use crate::layout::{LayoutItem, Protrusion};
use crate::theme::Globals;
use crate::ticks::Ticks;
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

/// Who a layout item belongs to.
enum Owner {
    Axis(usize),
    Block(BlockId),
}

/// Step 1 of a build: every block's layout request (axes after limits and ticks), in block order.
struct Collected {
    axes: Vec<AxisFrame>,
    items: Vec<LayoutItem>,
    owners: Vec<Owner>,
    /// Blocks drawn over an axis: (block, axis).
    inside: Vec<(BlockId, BlockId)>,
    /// Target limits (before `autolimitaspect`), by axis slot.
    targets: Vec<[f64; 4]>,
}

/// Major ticks for an axis with limits `lim` (categorical plots label their axis).
fn axis_ticks(st: &FigState, id: BlockId, attrs: &AxisResolved, lim: [f64; 4]) -> (Ticks, Ticks) {
    let mut xticks = crate::ticks::resolve_ticks(&attrs.xticks, &attrs.xtickformat, lim[0], lim[1], attrs.xscale);
    let mut yticks = crate::ticks::resolve_ticks(&attrs.yticks, &attrs.ytickformat, lim[2], lim[3], attrs.yscale);
    let plots = st.block(id).and_then(|b| b.as_axis()).map(|a| a.plots.as_slice()).unwrap_or(&[]);
    for pid in plots {
        if let Some((on_x, cats)) = st.plot(*pid).and_then(|p| p.kind.imp().categories()) {
            let t = Ticks {
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
    (xticks, yticks)
}

/// Sets an axis frame's limits (and everything derived from them: view, rebase, ticks).
fn set_limits(st: &FigState, a: &mut AxisFrame, lim: [f64; 4], cache: &mut SceneCache) {
    let at = &a.attrs;
    a.view =
        [at.xscale.forward(lim[0]), at.xscale.forward(lim[1]), at.yscale.forward(lim[2]), at.yscale.forward(lim[3])];
    a.limits = lim;
    a.rebase = cache.rebase_for(a.id, a.view);
    (a.xticks, a.yticks) = axis_ticks(st, a.id, &a.attrs, lim);
}

fn collect(st: &FigState, g: &Globals, cache: &mut SceneCache) -> Collected {
    let mut c = Collected { axes: vec![], items: vec![], owners: vec![], inside: vec![], targets: vec![] };
    let axis_ids: Vec<BlockId> =
        st.iter_blocks().filter(|(_, s)| matches!(s.block, Block::Axis(_))).map(|(id, _)| id).collect();
    let limits = axis::compute_limits(st, &axis_ids, g);
    for (id, bslot) in st.iter_blocks() {
        let ax = match &bslot.block {
            Block::Axis(ax) => ax,
            other => {
                let Some(imp) = other.imp() else { continue };
                if let Some(target) = imp.inside_axis() {
                    c.inside.push((id, target));
                    continue;
                }
                let ctx = crate::blocks::BlockCtx { st, g, axes: &[], id };
                let bl = imp.layout(&ctx);
                c.items.push(LayoutItem {
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
                c.owners.push(Owner::Block(id));
                continue;
            }
        };
        let slot = c.axes.len();
        let attrs = ax.attrs.resolve(&st.theme.axis, g);
        let lim = limits[slot];
        let mut frame = AxisFrame {
            id,
            slot: slot as u16,
            attrs,
            limits: lim,
            view: [0.0; 4],
            rebase: Rebase::for_view([0.0, 1.0, 0.0, 1.0], 0),
            xticks: Ticks::default(),
            yticks: Ticks::default(),
            rect: Rect::default(),
        };
        set_limits(st, &mut frame, lim, cache);
        let protrusion = axis::protrusion(&frame.attrs, &frame.xticks, &frame.yticks, lim);
        c.items.push(LayoutItem {
            rows: bslot.place.rows,
            cols: bslot.place.cols,
            side: bslot.place.side,
            protrusion,
            width: frame.attrs.width.into(),
            height: frame.attrs.height.into(),
            tellwidth: true,
            tellheight: true,
            // An axis with an aspect is rounded after it shrinks inside its cell.
            round: frame.attrs.aspect.is_none(),
            ..Default::default()
        });
        c.owners.push(Owner::Axis(slot));
        c.targets.push(lim);
        c.axes.push(frame);
    }
    c
}

/// Solves the layout and places the axes: Makie's `aspect` shrinks an axis inside its cell and
/// `autolimitaspect` widens its limits for the solved size. When the new limits change an axis'
/// protrusions (other tick labels), the layout is solved once more.
fn place(st: &FigState, g: &Globals, size: [f64; 2], c: &mut Collected, cache: &mut SceneCache) -> Vec<Rect> {
    let solve =
        |items: &[LayoutItem]| crate::layout::solve(items, &st.grid, size, g.figure_padding, g.colgap, g.rowgap);
    let mut rects = solve(&c.items);
    for pass in 0..2 {
        let mut relayout = false;
        for (i, o) in c.owners.iter().enumerate() {
            let Owner::Axis(s) = o else { continue };
            let a = &mut c.axes[*s];
            if a.attrs.autolimitaspect.is_some() {
                let r = rects[i];
                let target = c.targets[*s];
                // The limits aspect sees the area after `aspect` shrinks it (Makie's viewport).
                let area = axis::aspect_area(r, a.attrs.aspect, target, size[1]);
                let lim = axis::adjust_limits_for_aspect(target, &a.attrs, area.w, area.h);
                if lim != a.limits {
                    set_limits(st, a, lim, cache);
                    let p = axis::protrusion(&a.attrs, &a.xticks, &a.yticks, lim);
                    if p != c.items[i].protrusion {
                        c.items[i].protrusion = p;
                        relayout = true;
                    }
                }
            }
        }
        if !relayout || pass == 1 {
            break;
        }
        rects = solve(&c.items);
    }
    let mut block_rects = Vec::new();
    for (o, r) in c.owners.iter().zip(rects) {
        match o {
            Owner::Axis(s) => {
                let a = &mut c.axes[*s];
                a.rect =
                    if a.attrs.aspect.is_some() { axis::aspect_area(r, a.attrs.aspect, a.limits, size[1]) } else { r };
            }
            Owner::Block(_) => block_rects.push(r),
        }
    }
    block_rects
}

/// The figure size that fits its layout exactly (Makie's `resize_to_layout!`).
pub(crate) fn tight_size(st: &FigState) -> [f64; 2] {
    let g: Globals = st.theme.globals();
    let c = collect(st, &g, &mut SceneCache::new());
    crate::layout::tight_size(&c.items, &st.grid, g.size, g.figure_padding, g.colgap, g.rowgap)
}

/// Builds a frame. `size` overrides the figure size (window size).
pub(crate) fn build(st: &FigState, size: Option<[f64; 2]>, cache: &mut SceneCache) -> (DrawList, Vec<AxisFrame>) {
    let g: Globals = st.theme.globals();
    let size = size.unwrap_or(g.size);

    // 1. Layout requests, in block order: axes (after limits and ticks) and other blocks.
    let mut c = collect(st, &g, cache);

    // 2. Layout (and the limits that depend on it).
    let block_rects: Vec<(BlockId, Rect)> = {
        let rects = place(st, &g, size, &mut c, cache);
        let ids = c.owners.iter().filter_map(|o| if let Owner::Block(id) = o { Some(*id) } else { None });
        ids.zip(rects).collect()
    };
    let (axes, inside) = (c.axes, c.inside);

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
