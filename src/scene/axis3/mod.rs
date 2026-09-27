//! Lowering an `Axis3` (Makie's `makielayout/blocks/axis3d.jl`): limits, ticks, the camera,
//! decorations and 3D plots.
//!
//! Coordinates: Makie computes the decorations in figure pixels with y up; this module does the
//! same ([`Axis3Frame::project_up`]) and flips to sciplot's y-down figure units when emitting.
//! Plot data is stored as f32 *local* coordinates `(data - origin) * k` ([`Rebase3`], reused while
//! the limits stay comparable, so live data growth doesn't reconvert or reupload everything);
//! [`View3d`] maps local coordinates to the scene area.
//!
//! Draw order: the far panels, grid and frame lines (behind the data, as in Makie) are 2D items
//! below the plots; the plots are 3D items (`Prim::Lines3d` / `Markers3d` / `Mesh3d`) sharing one
//! depth buffer per axis: meshes (surfaces) first, writing depth, then lines and markers in plot
//! order, depth-tested against the meshes but not against each other (Makie's draw order, like
//! CairoMakie). Front spines, ticks and labels come after the plots.
//!
//! Window hooks (the window owns the event loop; nothing here depends on it):
//! - [`area_at`] finds the Axis3 under a cursor position from the last build's [`SceneCache`];
//! - [`drag_rotate`] (left drag) and [`scroll_zoom_limits`] / [`zoom_factor`] (scroll) are
//!   Makie's `DragRotate` / `ScrollZoom` as pure functions; `Axis3::rotate_by` and
//!   `Axis3::zoom_by` apply them to a figure.
//!
//! Provenance: algorithm adapted from Makie 0.24.14 `src/makielayout/blocks/axis3d.jl`
//! (`initialize_block!`: scene area, clip box nudge; `getlimits`) and
//! `src/makielayout/blocks/axis.jl` (`reset_limits!`, `expandlimits`). `drag_rotate`, `zoom_factor`
//! and `scroll_zoom_limits` are ported from `src/makielayout/interactions.jl` (`DragRotate`,
//! `ScrollZoom` for Axis3), and the default light follows `src/theming.jl`. `Rebase3` is original.
//! MIT licensed; see THIRD_PARTY_NOTICES.md.

pub(crate) mod camera;
mod decorations;
mod plotctx;

pub(crate) use plotctx::{Plot3dCtx, Plot3dImpl, Points3, plot3d};

use super::SceneCache;
use super::drawlist::{Emitter, Rect, View3d};
use crate::blocks::Block;
use crate::blocks::axis3::{Axis3Resolved, Axis3State};
use crate::figure::{BlockId, FigState};
use crate::theme::Globals;
use crate::ticks::Ticks;
use camera::{CameraParams, M4, Matrices, ViewMode};
use std::collections::HashMap;
use std::sync::Arc;

/// Per-render-context memo for Axis3s.
#[derive(Default)]
pub(crate) struct Axis3Cache {
    rebases: HashMap<BlockId, Rebase3>,
    epoch: u64,
    /// Every Axis3 of the last build: `(id, layout bbox, scene area)` in figure units (y down).
    pub frames: Vec<(BlockId, Rect, Rect)>,
    /// Figure size of the last build.
    pub size: [f64; 2],
    /// Append-aware local conversions of live 3D point data, by `(plot uid, part)`.
    pub(crate) append: HashMap<(u64, u8), plotctx::LocalCache3>,
}

/// Maps data to f32 local coordinates `(p - origin) * k` for one Axis3.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rebase3 {
    pub origin: [f64; 3],
    pub k: [f64; 3],
    /// Changes whenever origin or k do (cache key for converted data).
    pub epoch: u64,
}

impl Rebase3 {
    fn for_limits(l: [f64; 6], epoch: u64) -> Rebase3 {
        let mut origin = [0.0; 3];
        let mut k = [1.0; 3];
        for i in 0..3 {
            let (lo, hi) = (l[2 * i], l[2 * i + 1]);
            origin[i] = 0.5 * lo + 0.5 * hi;
            let w = hi - lo;
            k[i] = if w > 0.0 && (1.0 / w).is_finite() { 1.0 / w } else { 1.0 };
        }
        Rebase3 { origin, k, epoch }
    }

    /// Whether f32 local coordinates stay precise for limits `l` (centre within a few widths of
    /// the origin, width within a factor 8 of the one `k` was made for).
    fn adequate(&self, l: [f64; 6]) -> bool {
        (0..3).all(|i| {
            let (lo, hi) = (l[2 * i], l[2 * i + 1]);
            let c = (0.5 * lo + 0.5 * hi - self.origin[i]) * self.k[i];
            let w = (hi - lo) * self.k[i];
            c.abs() < 8.0 && w > 0.125 && w < 8.0
        })
    }

    pub fn to_local(self, p: [f64; 3]) -> [f32; 3] {
        std::array::from_fn(|i| ((p[i] - self.origin[i]) * self.k[i]) as f32)
    }
}

/// One Axis3 during a build.
pub(crate) struct Axis3Frame {
    pub id: BlockId,
    pub attrs: Axis3Resolved,
    /// Final limits `[x0, x1, y0, y1, z0, z1]`, ordered.
    pub limits: [f64; 6],
    pub ticks: [Ticks; 3],
    /// The layout rectangle (Makie's `computedbbox`), figure units, y down.
    pub bbox: Rect,
    /// The scene area: the bbox plus the protrusions, rounded to whole units (y down).
    pub area: Rect,
    /// Figure height (Makie's pixel space has y up).
    pub fig_h: f64,
    pub cam: Matrices,
    /// `projection * view * model`: data -> clip space of the scene area.
    pub pvm: M4,
    pub rebase: Rebase3,
}

impl Axis3Frame {
    /// Data -> figure pixels with y up (Makie's `project(blockscene, p)`), `None` behind the
    /// camera.
    pub fn project_up(&self, p: [f64; 3]) -> Option<[f64; 2]> {
        let q = camera::apply(&self.pvm, p);
        if !(q[3] > 0.0) {
            return None;
        }
        self.project_up_any(p)
    }

    /// [`project_up`](Self::project_up) without the camera-side check: points behind the camera
    /// come out mirrored, as in Makie's `project` (tick directions use such points).
    pub fn project_up_any(&self, p: [f64; 3]) -> Option<[f64; 2]> {
        let q = camera::apply(&self.pvm, p);
        if q[3] == 0.0 || !q.iter().all(|v| v.is_finite()) {
            return None;
        }
        let (nx, ny) = (q[0] / q[3], q[1] / q[3]);
        let a = self.area;
        let y_up0 = self.fig_h - a.bottom();
        Some([a.x + 0.5 * (nx + 1.0) * a.w, y_up0 + 0.5 * (ny + 1.0) * a.h])
    }

    /// Makie pixels (y up) -> figure units (y down).
    pub fn down(&self, p: [f64; 2]) -> [f64; 2] {
        [p[0], self.fig_h - p[1]]
    }

    /// The camera as seen by 3D primitives (local coordinates).
    pub fn view3d(&self) -> View3d {
        let m = &self.cam.model;
        let rb = &self.rebase;
        // data = local / k + origin.
        let mut linv = camera::identity();
        for i in 0..3 {
            linv[i][i] = 1.0 / rb.k[i];
            linv[i][3] = rb.origin[i];
        }
        let mvp = camera::mul(&self.pvm, &linv);
        let world_scale: [f64; 3] = std::array::from_fn(|i| m[i][i] / rb.k[i]);
        let world_offset: [f64; 3] = std::array::from_fn(|i| m[i][i] * rb.origin[i] + m[i][3]);
        let normal_scale: [f64; 3] = std::array::from_fn(|i| 1.0 / m[i][i]);
        let clip = self.attrs.clip.then(|| {
            let l = self.limits;
            let lo: [f32; 3] = std::array::from_fn(|i| {
                let w = (l[2 * i + 1] - l[2 * i]) * rb.k[i];
                ((l[2 * i] - rb.origin[i]) * rb.k[i] - 1e-5 * w) as f32
            });
            let hi: [f32; 3] = std::array::from_fn(|i| {
                let w = (l[2 * i + 1] - l[2 * i]) * rb.k[i];
                ((l[2 * i + 1] - rb.origin[i]) * rb.k[i] + 1e-5 * w) as f32
            });
            [lo, hi]
        });
        // Makie's default light: camera-relative direction (-0.457, -0.629, -0.629) in eye space,
        // turned into world space by the inverse view rotation.
        let dir = LIGHT_DIRECTION;
        let v = &self.cam.view;
        let light: [f64; 3] = std::array::from_fn(|i| (0..3).map(|j| v[j][i] * dir[j]).sum());
        View3d {
            mvp,
            area: self.area,
            world_scale,
            world_offset,
            normal_scale,
            clip,
            light_dir: light,
            eye: self.cam.eyepos,
            ambient: AMBIENT,
            light_color: LIGHT_COLOR,
            group: self.id.index as u64 + 1,
        }
    }
}

/// Makie's default `light_direction` (camera relative), `ambient` and `light_color`.
pub(crate) const LIGHT_DIRECTION: [f64; 3] = [-0.45679495, -0.6293204, -0.6287243];
pub(crate) const AMBIENT: f32 = 0.45;
pub(crate) const LIGHT_COLOR: f32 = 0.5;

/// Makie's `(1 - speed)^amount` zoom multiplier of `ScrollZoom(0.05)`.
pub fn zoom_factor(amount: f64) -> f64 {
    (1.0f64 - 0.05).powf(amount)
}

/// Makie's `ScrollZoom` for Axis3 (`zoommode = :center`): `limits` scaled about their centre by
/// [`zoom_factor`]`(amount)`; `amount > 0` zooms in.
pub fn scroll_zoom_limits(limits: [f64; 6], amount: f64) -> [f64; 6] {
    let m = zoom_factor(amount);
    let mut out = limits;
    for i in 0..3 {
        let c = 0.5 * (limits[2 * i] + limits[2 * i + 1]);
        out[2 * i] = c + m * (limits[2 * i] - c);
        out[2 * i + 1] = c + m * (limits[2 * i + 1] - c);
    }
    out
}

/// Makie's `DragRotate`: a drag by `(dx, dy)` units (y down, as the cursor moves) turns the
/// azimuth by `-0.01 dx` and the elevation by `+0.01 dy` (Makie's `-0.01 dy` in its y-up pixels),
/// the elevation clamped to `±(π/2 - 0.001)`. Returns `(azimuth, elevation)`.
pub fn drag_rotate(azimuth: f64, elevation: f64, dx: f64, dy: f64) -> (f64, f64) {
    let lim = std::f64::consts::FRAC_PI_2 - 0.001;
    (azimuth - 0.01 * dx, (elevation + 0.01 * dy).clamp(-lim, lim))
}

/// The Axis3 whose scene area contains `p` (figure units, y down) in the last build of `cache`.
pub(crate) fn area_at(cache: &SceneCache, p: [f64; 2]) -> Option<BlockId> {
    cache.axis3.frames.iter().rev().find(|(_, _, a)| a.contains(p)).map(|(id, _, _)| *id)
}

/// Final limits of an Axis3 (Makie's `reset_limits!`): interactive limits if any, otherwise
/// the data bounds of the visible plots plus `autolimitmargin` (or `(0, 1)` without data), with
/// the user's `xlims`/`ylims`/`zlims` taking precedence; ordered, never empty.
pub(crate) fn final_limits(st: &FigState, a: &Axis3State, g: &Globals) -> [f64; 6] {
    if let Some(l) = a.interactive {
        return l;
    }
    let r = a.attrs.resolve(&st.theme.axis3, g);
    let mut b: [Option<(f64, f64)>; 3] = [None; 3];
    for pid in &a.plots {
        let Some(p) = st.plot(*pid) else { continue };
        if !p.common.visible {
            continue;
        }
        let Some(imp) = plot3d(&p.kind) else { continue };
        let Some(bb) = imp.bounds() else { continue };
        let auto = [p.common.xautolimits, p.common.yautolimits, true];
        for i in 0..3 {
            let (lo, hi) = (bb[2 * i], bb[2 * i + 1]);
            if auto[i] && lo.is_finite() && hi.is_finite() && lo <= hi {
                b[i] = Some(match b[i] {
                    Some((l0, h0)) => (l0.min(lo), h0.max(hi)),
                    None => (lo, hi),
                });
            }
        }
    }
    let margins = [r.xautolimitmargin, r.yautolimitmargin, r.zautolimitmargin];
    let id = crate::transform::Scale::Identity;
    let mut out = [0.0; 6];
    for i in 0..3 {
        let (mut lo, mut hi) = match b[i] {
            Some(bi) => super::axis::expand(bi, margins[i], id),
            None => (0.0, 1.0),
        };
        if let Some(v) = a.limits[2 * i] {
            lo = v;
        }
        if let Some(v) = a.limits[2 * i + 1] {
            hi = v;
        }
        if lo > hi {
            std::mem::swap(&mut lo, &mut hi);
        }
        if !(lo < hi) || !lo.is_finite() || !hi.is_finite() {
            let c = if lo.is_finite() { lo } else { 0.0 };
            (lo, hi) = super::axis::expand((c, c), [0.0; 2], id);
        }
        out[2 * i] = lo;
        out[2 * i + 1] = hi;
    }
    out
}

/// Lays out one Axis3 in `bbox` (figure units, y down) of a `size` figure.
pub(crate) fn frame(
    st: &FigState,
    g: &Globals,
    id: BlockId,
    a: &Axis3State,
    bbox: Rect,
    size: [f64; 2],
    cache: &mut SceneCache,
) -> Axis3Frame {
    let attrs = a.attrs.resolve(&st.theme.axis3, g);
    let limits = final_limits(st, a, g);
    let ticks = [
        crate::ticks::resolve_ticks(
            &attrs.xticks,
            &attrs.xtickformat,
            limits[0],
            limits[1],
            crate::transform::Scale::Identity,
        ),
        crate::ticks::resolve_ticks(
            &attrs.yticks,
            &attrs.ytickformat,
            limits[2],
            limits[3],
            crate::transform::Scale::Identity,
        ),
        crate::ticks::resolve_ticks(
            &attrs.zticks,
            &attrs.ztickformat,
            limits[4],
            limits[5],
            crate::transform::Scale::Identity,
        ),
    ];
    let fig_h = size[1];
    // Makie's scene area: `round_to_IRect2D(bbox + protrusions)` in y-up pixels.
    let [pl, pr, pb, pt] = attrs.protrusions;
    let up_y0 = fig_h - bbox.bottom();
    let (x0, y0) = ((bbox.x - pl).round(), (up_y0 - pb).round());
    let (x1, y1) = ((bbox.right() + pr).round(), (up_y0 + bbox.h + pt).round());
    let area = Rect::new(x0, fig_h - y1, (x1 - x0).max(1.0), (y1 - y0).max(1.0));
    let cam = camera::calculate_matrices(&CameraParams {
        limits,
        viewport: [area.w, area.h],
        protrusions: attrs.protrusions,
        elevation: attrs.elevation,
        azimuth: attrs.azimuth,
        perspectiveness: attrs.perspectiveness,
        aspect: attrs.aspect,
        viewmode: attrs.viewmode,
        reversed: [attrs.xreversed, attrs.yreversed, attrs.zreversed],
        zoom_mult: if attrs.viewmode == ViewMode::Free { a.zoom_mult } else { 1.0 },
        offset: a.offset,
        near: attrs.near,
    });
    let pvm = camera::mul(&cam.projection, &camera::mul(&cam.view, &cam.model));
    let c3 = &mut cache.axis3;
    let rebase = match c3.rebases.get(&id) {
        Some(r) if r.adequate(limits) => *r,
        _ => {
            c3.epoch += 1;
            let r = Rebase3::for_limits(limits, c3.epoch);
            c3.rebases.insert(id, r);
            r
        }
    };
    Axis3Frame { id, attrs, limits, ticks, bbox, area, fig_h, cam, pvm, rebase }
}

/// Draws one Axis3 (decorations and plots) into its layout rectangle `bbox`.
pub(crate) fn emit(
    st: &FigState,
    g: &Globals,
    id: BlockId,
    bbox: Rect,
    size: [f64; 2],
    cache: &mut SceneCache,
    em: &mut Emitter,
) {
    let Some(Block::Axis3(a)) = st.block(id) else { return };
    let f = frame(st, g, id, a, bbox, size, cache);
    cache.axis3.frames.push((id, bbox, f.area));
    cache.axis3.size = size;
    decorations::emit_back(em, &f);
    emit_plots(st, g, a, &f, cache, em);
    decorations::emit_front(em, &f);
}

/// Lowers the plots of an Axis3: meshes first (they write depth), then the rest in plot order.
fn emit_plots(st: &FigState, g: &Globals, a: &Axis3State, f: &Axis3Frame, cache: &mut SceneCache, em: &mut Emitter) {
    let view = Arc::new(f.view3d());
    let cycles = super::cycle_indices(st, &a.plots);
    for meshes in [true, false] {
        for (pid, &cycle) in a.plots.iter().zip(&cycles) {
            let Some(p) = st.plot(*pid) else { continue };
            if !p.common.visible {
                continue;
            }
            let Some(imp) = plot3d(&p.kind) else { continue };
            if imp.is_mesh() != meshes {
                continue;
            }
            let mut ctx = Plot3dCtx {
                em: &mut *em,
                frame: f,
                view: view.clone(),
                g,
                theme: &st.theme,
                cache: &mut *cache,
                uid: p.uid,
                data_rev: p.data_rev,
                cycle,
                z: p.common.z,
            };
            imp.emit3(&mut ctx);
        }
    }
}

/// An Axis3 as laid out (Makie's conventions: figure pixels with y up), for fidelity checks.
#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Axis3Geometry {
    /// Layout bbox `[x, y, w, h]` (y up).
    pub bbox: [f64; 4],
    /// Scene area `[x, y, w, h]` (y up).
    pub viewport: [f64; 4],
    /// Final limits `[x0, x1, y0, y1, z0, z1]`.
    pub limits: [f64; 6],
    /// Row-major camera matrices.
    pub model: [[f64; 4]; 4],
    pub view: [[f64; 4]; 4],
    pub projection: [[f64; 4]; 4],
    pub eyeposition: [f64; 3],
    /// Per dimension: tick values, tick segments `[start, end]`, tick label anchors, the tick
    /// label alignment `(h, v)` (0 = left/bottom .. 1 = right/top), the axis label anchor, its
    /// rotation and alignment.
    pub dims: [DimGeometry; 3],
    /// Grid line and frame line endpoints in data space, per dimension: `(grid1, grid2, frame)`.
    pub lines3d: [(Vec<[f64; 3]>, Vec<[f64; 3]>, Vec<[f64; 3]>); 3],
    /// Title anchor.
    pub title: [f64; 2],
}

/// One dimension of an [`Axis3Geometry`].
#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DimGeometry {
    pub tickvalues: Vec<f64>,
    pub ticks: Vec<[[f64; 2]; 2]>,
    pub ticklabel_pos: Vec<[f64; 2]>,
    pub ticklabel_align: (f64, f64),
    pub label_pos: [f64; 2],
    pub label_rot: f64,
    pub label_align: (f64, f64),
}

impl Axis3Geometry {
    /// Projects a data point like Makie's `project(blockscene, p)` (figure pixels, y up).
    pub fn project(&self, p: [f64; 3]) -> [f64; 2] {
        let pvm = camera::mul(&self.projection, &camera::mul(&self.view, &self.model));
        let q = camera::apply(&pvm, p);
        let [x, y, w, h] = self.viewport;
        [x + 0.5 * (q[0] / q[3] + 1.0) * w, y + 0.5 * (q[1] / q[3] + 1.0) * h]
    }
}

pub(crate) fn geometry(st: &FigState, cache: &SceneCache, id: BlockId) -> Option<Axis3Geometry> {
    let Some(Block::Axis3(a)) = st.block(id) else { return None };
    let (_, bbox, _) = cache.axis3.frames.iter().find(|(i, _, _)| *i == id)?;
    let g = st.theme.globals();
    let mut scratch = SceneCache::new();
    let f = frame(st, &g, id, a, *bbox, cache.axis3.size, &mut scratch);
    let up = |r: Rect| [r.x, f.fig_h - r.bottom(), r.w, r.h];
    let mut out = Axis3Geometry {
        bbox: up(f.bbox),
        viewport: up(f.area),
        limits: f.limits,
        model: f.cam.model,
        view: f.cam.view,
        projection: f.cam.projection,
        eyeposition: f.cam.eyepos,
        title: decorations::title_anchor(&f),
        ..Default::default()
    };
    for d in 0..3 {
        let t = decorations::ticks(&f, d);
        let l = decorations::label(&f, d);
        out.dims[d] = DimGeometry {
            tickvalues: f.ticks[d].values.clone(),
            ticks: t.segments.clone(),
            ticklabel_pos: t.label_pos.clone(),
            ticklabel_align: t.align,
            label_pos: l.pos,
            label_rot: l.rotation,
            label_align: l.align,
        };
        let (g1, g2) = decorations::gridlines(&f, d);
        out.lines3d[d] = (g1, g2, decorations::framelines(&f, d).0);
    }
    Some(out)
}
