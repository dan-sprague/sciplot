//! `heatmap`: a grid of cells colored through a colormap (Makie's `Heatmap`).
//!
//! Values are converted once, on the caller's thread, to f32 `(v - off) * k` with `off`/`k` from
//! the f64 extrema, so fields of any magnitude keep their resolution; the GPU draws one quad and
//! looks the cell up per fragment.
//!
//! Provenance: defaults and behaviour follow Makie 0.24.14 `src/basic_plots.jl`
//! (`@recipe Heatmap`), the legend element of `src/makielayout/types.jl`
//! (`heatmapvalues = [0 0.3; 0.6 1]`) and the cell outline and NaN rule of `show_imagelike` in
//! `src/interaction/inspector.jl`. MIT licensed; see THIRD_PARTY_NOTICES.md.

use super::{PlotImpl, PlotKind, add_to_axis, plot_common};
use crate::attrs::attributes;
use crate::color::{Color, Colormap, MappingAttrs, ValueEncoding};
use crate::data::{CellCoords, CellEdges, CellSpecKind as Spec, Data2D};
use crate::figure::{Dirty, FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::scene::drawlist::{Buf, BufKey, FieldPrim, GridAxis, Prim};
use crate::transform::Scale;
use parking_lot::Mutex;
use std::sync::Arc;

/// A heatmap plot handle (Makie's `Heatmap`).
#[derive(Clone)]
pub struct Heatmap {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct HeatmapState {
    pub xspec: Spec,
    pub yspec: Spec,
    /// Cell edges in data space.
    pub x: CellEdges,
    pub y: CellEdges,
    pub nx: usize,
    pub ny: usize,
    /// `nx * ny` encoded values, x fastest.
    pub values: Arc<Vec<f32>>,
    pub enc: ValueEncoding,
    /// A replaced value buffer kept for the next `set_data` (never the current one).
    pub spare: Arc<Mutex<Option<Vec<f32>>>>,
    pub attrs: HeatmapAttrs,
}

attributes! {
    Heatmap(HeatmapAttrs, HeatmapResolved, HeatmapTheme) via with_attrs {
        /// `Colormap::MAGMA`, a name like `"magma"`, or a list of colors (default viridis).
        colormap: Colormap = |_| Colormap::VIRIDIS, STYLE;
        /// `(lo, hi)` mapped to the colormap ends; default: the finite extrema of the data.
        colorrange: Option<[f64; 2]> = |_| None, STYLE;
        /// Color for values below the colorrange (default: the first colormap color).
        lowclip: Option<Color> = |_| None, STYLE;
        /// Color for values above the colorrange (default: the last colormap color).
        highclip: Option<Color> = |_| None, STYLE;
        /// Color for NaN cells (default transparent).
        nan_color: Color = |_| Color::TRANSPARENT, STYLE;
        /// Bilinear interpolation between cell centres (default false: flat cells).
        interpolate: bool = |_| false, STYLE;
        /// Opacity multiplier.
        alpha: f64 = |_| 1.0, STYLE;
    }
}

plot_common!(Heatmap);
super::color_mapped!(Heatmap);

/// Finite bounds of the cell edges in scaled space.
fn edge_bounds(e: &CellEdges, s: Scale) -> Option<(f64, f64)> {
    let n = e.n();
    if n == 0 {
        return None;
    }
    let ends = matches!(e, CellEdges::Regular { .. }) && s == Scale::Identity;
    let it: Box<dyn Iterator<Item = f64>> =
        if ends { Box::new([e.first(), e.last()].into_iter()) } else { Box::new((0..=n).map(|i| e.edge(i))) };
    crate::data::finite_extrema(it.map(|v| s.forward(v)))
}

impl HeatmapState {
    /// Cell edges along one axis as the GPU sees them (local coordinates).
    fn grid_axis(&self, ctx: &mut PlotCtx<'_>, part: u8, dim: usize) -> GridAxis {
        let (e, scale) = if dim == 0 { (&self.x, ctx.axis.attrs.xscale) } else { (&self.y, ctx.axis.attrs.yscale) };
        let (o, k) = (ctx.axis.rebase.origin[dim], ctx.axis.rebase.k[dim]);
        let local = |v: f64| (scale.forward(v) - o) * k;
        if let (CellEdges::Regular { e0, e1, .. }, Scale::Identity) = (e, scale) {
            return GridAxis::Regular { e0: local(*e0), e1: local(*e1) };
        }
        let mut v: Vec<f32> = (0..=e.n()).map(|i| local(e.edge(i)) as f32).collect();
        // Edges outside the scale's domain (e.g. <= 0 on a log axis) collapse onto their
        // neighbours, leaving empty cells.
        if let Some(f) = v.iter().position(|x| x.is_finite()) {
            let first = v[f];
            v[..f].fill(first);
            for i in f + 1..v.len() {
                if !v[i].is_finite() {
                    v[i] = v[i - 1];
                }
            }
        }
        GridAxis::Edges(Buf { key: Some(BufKey { uid: ctx.uid, part, rev: ctx.conv_key(part) }), data: Arc::new(v) })
    }
}

impl PlotImpl for HeatmapState {
    fn cycle_group(&self) -> &'static str {
        "heatmap"
    }

    fn color_is_auto(&self, _theme: &crate::theme::Theme) -> bool {
        false
    }

    fn data_bounds(&self, xs: Scale, ys: Scale) -> Option<[f64; 4]> {
        let (x0, x1) = edge_bounds(&self.x, xs)?;
        let (y0, y1) = edge_bounds(&self.y, ys)?;
        Some([x0, x1, y0, y1])
    }

    fn tight_limits(&self) -> bool {
        true
    }

    fn legend_elements(&self, ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        // Makie's heatmap element: values [0 0.3; 0.6 1] (first index = x) through the colormap.
        let r = self.attrs.resolve(&ctx.theme.heatmap, ctx.g);
        let c = |t: f64| {
            let c = r.colormap.sample(t);
            c.with_alpha(c.a * r.alpha as f32)
        };
        vec![super::LegendElement::Cells { colors: [c(0.0), c(0.6), c(0.3), c(1.0)] }]
    }

    fn colormapping(&self, theme: &crate::theme::Theme, g: &crate::theme::Globals) -> Option<super::ResolvedColormap> {
        let r = self.attrs.resolve(&theme.heatmap, g);
        let [lo, hi] = r.colorrange.unwrap_or_else(|| self.enc.auto_range());
        Some(super::ResolvedColormap {
            colormap: r.colormap,
            colorrange: (lo, hi),
            lowclip: r.lowclip,
            highclip: r.highclip,
            alpha: r.alpha,
            mapped: true,
        })
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        if self.nx == 0 || self.ny == 0 {
            return;
        }
        let r = self.attrs.resolve(&ctx.theme.heatmap, ctx.g);
        let map = MappingAttrs {
            colormap: &r.colormap,
            colorrange: r.colorrange,
            lowclip: r.lowclip,
            highclip: r.highclip,
            nan_color: r.nan_color,
            alpha: r.alpha,
        }
        .mapping(&self.enc);
        let x = self.grid_axis(ctx, 1, 0);
        let y = self.grid_axis(ctx, 2, 1);
        let values = ctx.data_buf(0, self.values.clone());
        ctx.push_data(Prim::Field(FieldPrim {
            values,
            nx: self.nx as u32,
            ny: self.ny as u32,
            x,
            y,
            map,
            interpolate: r.interpolate,
        }));
    }

    fn pick(&self, ctx: &mut super::pick::PickCtx<'_>) -> Option<super::pick::Hover> {
        self.pick_cell(ctx)
    }
}

/// The cell of `e` containing `v` (data space), for monotone edges in either direction.
fn cell_index(e: &CellEdges, v: f64) -> Option<usize> {
    let n = e.n();
    if n == 0 || !v.is_finite() {
        return None;
    }
    let (first, last) = (e.first(), e.last());
    let (lo, hi) = (first.min(last), first.max(last));
    if !(v >= lo && v <= hi) {
        return None;
    }
    let up = last >= first;
    // Binary search for the last edge at or before v (in the direction of the edges).
    let before = |i: usize| if up { e.edge(i) <= v } else { e.edge(i) >= v };
    let (mut a, mut b) = (0usize, n); // before(a) holds; find the largest such index < n
    while b - a > 1 {
        let m = (a + b) / 2;
        if before(m) { a = m } else { b = m }
    }
    Some(a.min(n - 1))
}

impl HeatmapState {
    /// Makie's heatmap inspection: the cell under the cursor, as `x = …, y = …` (cursor, data
    /// space) and `[i, j] = v` (0-based indices, decoded value), with the cell outlined in red.
    fn pick_cell(&self, ctx: &mut super::pick::PickCtx<'_>) -> Option<super::pick::Hover> {
        let [x, y] = ctx.cursor_data()?;
        let i = cell_index(&self.x, x)?;
        let j = cell_index(&self.y, y)?;
        let raw = *self.values.get(j * self.nx + i)?;
        let v = if raw.is_nan() { f64::NAN } else { raw as f64 / self.enc.k + self.enc.off };
        let r = self.attrs.resolve(&ctx.theme.heatmap, ctx.g);
        if v.is_nan() && r.nan_color.a <= 0.0 {
            return None;
        }
        let a = ctx.axis.to_units(self.x.edge(i), self.y.edge(j));
        let b = ctx.axis.to_units(self.x.edge(i + 1), self.y.edge(j + 1));
        let outline = a.zip(b).map(|(a, b)| {
            let (x0, y0) = (a[0].min(b[0]), a[1].min(b[1]));
            [x0, y0, (a[0] - b[0]).abs(), (a[1] - b[1]).abs()]
        });
        use super::pick::sig6;
        Some(super::pick::Hover {
            // Anything else within the pick radius (drawn over the heatmap) wins.
            dist: ctx.radius,
            anchor: ctx.cursor,
            text: format!("x = {}, y = {}\n[{i}, {j}] = {}", sig6(x), sig6(y), sig6(v)),
            ring: None,
            outline,
        })
    }
}

/// Converts `z` (off-lock) into an encoded buffer, reusing `buf`'s allocation.
fn encode(z: &impl Data2D, mut buf: Vec<f32>) -> (Vec<f32>, ValueEncoding) {
    let enc = ValueEncoding::new(z.extrema());
    z.write_f32(&mut buf, enc.off, enc.k);
    (buf, enc)
}

#[track_caller]
fn edges_or_panic(what: &str, spec: &Spec, n: usize) -> CellEdges {
    spec.edges(n).unwrap_or_else(|e| panic!("{what}: {e}"))
}

impl Heatmap {
    fn with_attrs(&self, f: impl FnOnce(&mut HeatmapAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Heatmap(s) = &mut p.kind {
                f(&mut s.attrs)
            }
        });
    }

    #[track_caller]
    pub(crate) fn create(ax: &crate::Axis, xspec: Spec, yspec: Spec, z: impl Data2D) -> Heatmap {
        let (nx, ny) = z.dims();
        let x = edges_or_panic("heatmap x", &xspec, nx);
        let y = edges_or_panic("heatmap y", &yspec, ny);
        let (values, enc) = encode(&z, Vec::new());
        let st = HeatmapState {
            xspec,
            yspec,
            x,
            y,
            nx,
            ny,
            values: Arc::new(values),
            enc,
            spare: Arc::default(),
            attrs: HeatmapAttrs::default(),
        };
        let id = add_to_axis(ax, PlotKind::Heatmap(st));
        Heatmap { sh: ax.sh.clone(), id }
    }

    /// Replaces the values. The conversion runs on the calling thread; a field with the same
    /// dimensions reuses the previous buffers. New dimensions recompute the cell edges from the
    /// coordinates the heatmap was created with (vector coordinates must then match).
    ///
    /// # Panics
    /// If the dimensions changed and explicit coordinate vectors no longer fit.
    #[track_caller]
    pub fn set_data(&self, z: impl Data2D) -> Heatmap {
        let (nx, ny) = z.dims();
        let spare = match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Heatmap(s)) => s.spare.clone(),
            _ => Arc::default(),
        };
        let recycled = spare.lock().take().filter(|v| v.capacity() >= nx * ny).unwrap_or_default();
        let (values, enc) = encode(&z, recycled);
        let values = Arc::new(values);
        let res = self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| -> Result<_, String> {
            let PlotKind::Heatmap(s) = &mut p.kind else { return Ok(None) };
            if (nx, ny) != (s.nx, s.ny) {
                s.x = s.xspec.edges(nx).map_err(|e| format!("Heatmap::set_data x: {e}"))?;
                s.y = s.yspec.edges(ny).map_err(|e| format!("Heatmap::set_data y: {e}"))?;
                (s.nx, s.ny) = (nx, ny);
            }
            s.enc = enc;
            p.data_rev += 1;
            Ok(Some(std::mem::replace(&mut s.values, values)))
        });
        match res {
            Some(Err(e)) => panic!("{e}"),
            Some(Ok(Some(old))) => {
                // Recycle only if no render snapshot still holds it.
                if let Ok(v) = Arc::try_unwrap(old) {
                    *spare.lock() = Some(v);
                }
            }
            _ => {}
        }
        self.clone()
    }

    /// Replaces the cell coordinates (same rules as [`Axis::heatmap_xy`](crate::Axis::heatmap_xy)).
    ///
    /// # Panics
    /// If a coordinate vector's length is neither `n` (centres) nor `n + 1` (edges).
    #[track_caller]
    pub fn set_coords(&self, x: impl CellCoords, y: impl CellCoords) -> Heatmap {
        let (xs, ys) = (x.cell_spec().0, y.cell_spec().0);
        let res = self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| -> Result<_, String> {
            let PlotKind::Heatmap(s) = &mut p.kind else { return Ok(()) };
            s.x = xs.edges(s.nx).map_err(|e| format!("Heatmap::set_coords x: {e}"))?;
            s.y = ys.edges(s.ny).map_err(|e| format!("Heatmap::set_coords y: {e}"))?;
            (s.xspec, s.yspec) = (xs, ys);
            p.data_rev += 1;
            Ok(())
        });
        if let Some(Err(e)) = res {
            panic!("{e}");
        }
        self.clone()
    }

    /// The colorrange in effect: the explicit (or themed) `colorrange`, else the finite extrema of
    /// the data (widened by ±0.5 when all values are equal, like Makie).
    pub fn resolved_colorrange(&self) -> (f64, f64) {
        let st = self.sh.state.lock();
        match st.plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Heatmap(s)) => {
                let [lo, hi] =
                    s.attrs.colorrange.or(st.theme.heatmap.colorrange).flatten().unwrap_or(s.enc.auto_range());
                (lo, hi)
            }
            _ => (0.0, 1.0),
        }
    }

    /// `(nx, ny)`: the number of cells along x and y.
    pub fn dims(&self) -> (usize, usize) {
        match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Heatmap(s)) => (s.nx, s.ny),
            _ => (0, 0),
        }
    }
}

impl crate::Axis {
    /// Makie's `heatmap!(ax, z)`: cell `(i, j)` (0-based) is centred on `(i + 1, j + 1)`.
    #[track_caller]
    pub fn heatmap(&self, z: impl Data2D) -> Heatmap {
        Heatmap::create(self, Spec::Index, Spec::Index, z)
    }

    /// Makie's `heatmap!(ax, x, y, z)`. `x` and `y` are [`CellCoords`]: `a..=b` gives the centres of
    /// the first and last cell, [`Edges(a, b)`](crate::Edges) the outer edges, and a vector of `n`
    /// centres or `n + 1` edges gives each cell. `z[i, j]` is the cell at `(x_i, y_j)`.
    ///
    /// ```no_run
    /// use sciplot::prelude::*;
    /// let (nx, ny) = (64, 32);
    /// let v: Vec<f64> = (0..nx * ny).map(|k| ((k % nx) as f64 * 0.2).sin()).collect();
    /// let fig = Figure::new();
    /// let ax = Axis::new(fig.at(1, 1));
    /// ax.heatmap_xy(Edges(0.0, 2.0), Edges(0.0, 1.0), Field::new(&v, nx, ny)).colormap(Colormap::MAGMA);
    /// ```
    #[track_caller]
    pub fn heatmap_xy(&self, x: impl CellCoords, y: impl CellCoords, z: impl Data2D) -> Heatmap {
        Heatmap::create(self, x.cell_spec().0, y.cell_spec().0, z)
    }
}

impl crate::GridPosition {
    /// Makie's `heatmap(fig[r, c], z)`: a new Axis at this position with a heatmap.
    #[track_caller]
    pub fn heatmap(&self, z: impl Data2D) -> Heatmap {
        crate::Axis::new(self.clone()).heatmap(z)
    }

    /// Makie's `heatmap(fig[r, c], x, y, z)`: a new Axis at this position with a heatmap.
    #[track_caller]
    pub fn heatmap_xy(&self, x: impl CellCoords, y: impl CellCoords, z: impl Data2D) -> Heatmap {
        crate::Axis::new(self.clone()).heatmap_xy(x, y, z)
    }
}

/// Makie's `heatmap(z)`: a new Figure and Axis with a heatmap (cells centred on `1..=nx`,
/// `1..=ny`). Returns the plot handle; call `.save(..)` or `.show()` on it.
///
/// ```no_run
/// let z: Vec<f64> = (0..100 * 50).map(|k| (k as f64 * 0.01).sin()).collect();
/// sciplot::heatmap(sciplot::Field::new(&z, 100, 50)).save("heatmap.png").unwrap();
/// ```
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn heatmap(z: impl Data2D) -> Heatmap {
    crate::Figure::new().at(1, 1).heatmap(z)
}

/// Makie's `heatmap(x, y, z)`: a new Figure and Axis with a heatmap on the given cell coordinates
/// (see [`Axis::heatmap_xy`](crate::Axis::heatmap_xy)).
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn heatmap_xy(x: impl CellCoords, y: impl CellCoords, z: impl Data2D) -> Heatmap {
    crate::Figure::new().at(1, 1).heatmap_xy(x, y, z)
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::scene::SceneCache;

    fn limits(fig: &Figure) -> [f64; 4] {
        let (_, axes) = crate::scene::build(&fig.sh.snapshot(), None, &mut SceneCache::new());
        axes[0].limits
    }

    fn close(a: [f64; 4], b: [f64; 4]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-12)
    }

    /// Confirmed with CairoMakie (tools/heatmap_check.jl): heatmap(0..1, 0..1, 4x3 matrix) has
    /// limits x = (-1/6, 7/6), y = (-0.25, 1.25).
    #[test]
    fn interval_coordinates_give_makie_limits() {
        let z = vec![0.0; 12];
        let hm = heatmap_xy(0.0..=1.0, 0.0..=1.0, Field::new(&z, 4, 3));
        assert!(close(limits(&hm.figure()), [-1.0 / 6.0, 7.0 / 6.0, -0.25, 1.25]), "{:?}", limits(&hm.figure()));
        let hm = heatmap(Field::new(&z, 4, 3));
        assert!(close(limits(&hm.figure()), [0.5, 4.5, 0.5, 3.5]));
        let hm = heatmap_xy(Edges(0, 2), [0.0, 1.0, 3.0, 4.0], Field::new(&z, 4, 3));
        assert!(close(limits(&hm.figure()), [0.0, 2.0, 0.0, 4.0]));
    }

    #[test]
    fn pick_reports_the_cell_under_the_cursor() {
        // 3 × 2 cells on Edges(0, 3) × Edges(0, 2); z[i, j] = 1e6 + 10 i + j.
        let z: Vec<f64> = (0..6).map(|k| 1e6 + 10.0 * (k % 3) as f64 + (k / 3) as f64).collect();
        let mut hm = heatmap_xy(Edges(0.0, 3.0), Edges(0.0, 2.0), Field::new(&z, 3, 2));
        hm = hm.nan_color(RED);
        let st = hm.sh.snapshot();
        let (_, axes) = crate::scene::build(&st, None, &mut SceneCache::new());
        let a = &axes[0];
        let g = st.theme.globals();
        let mut cache = crate::plots::pick::PickCache::default();
        let p = st.plots[0].as_ref().unwrap();
        let mut at = |x: f64, y: f64| {
            let mut ctx = crate::plots::pick::PickCtx {
                axis: a,
                cursor: a.to_units(x, y).unwrap(),
                radius: 10.0,
                uid: p.uid,
                data_rev: p.data_rev,
                theme: &st.theme,
                g: &g,
                cache: &mut cache,
            };
            p.kind.imp().pick(&mut ctx)
        };
        let h = at(2.5, 0.25).expect("on the heatmap");
        assert_eq!(h.text, "x = 2.5, y = 0.25\n[2, 0] = 1.00002e+06");
        let [x0, y0] = a.to_units(2.0, 1.0).unwrap();
        let [x1, y1] = a.to_units(3.0, 0.0).unwrap();
        let o = h.outline.unwrap();
        assert!((o[0] - x0).abs() < 1e-9 && (o[1] - y0).abs() < 1e-9);
        assert!((o[2] - (x1 - x0)).abs() < 1e-9 && (o[3] - (y1 - y0)).abs() < 1e-9);
        assert!(at(0.5, 1.5).unwrap().text.ends_with("[0, 1] = 1e+06"));
        drop(hm);
    }

    #[test]
    fn set_data_and_colorrange() {
        let z = vec![1.0, 2.0, 3.0, 4.0];
        let hm = heatmap(Field::new(&z, 2, 2));
        assert_eq!(hm.resolved_colorrange(), (1.0, 4.0));
        hm.set_data(Field::new(&[5.0; 6], 3, 2));
        assert_eq!(hm.dims(), (3, 2));
        assert_eq!(hm.resolved_colorrange(), (4.5, 5.5));
        hm.colorrange((0, 10));
        assert_eq!(hm.resolved_colorrange(), (0.0, 10.0));
        hm.set_coords(Edges(0.0, 3.0), 0.0..=1.0);
        assert!(close(limits(&hm.figure()), [0.0, 3.0, -0.5, 1.5]));
        let bad = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hm.set_coords([1.0, 2.0], 0.0..=1.0)));
        assert!(bad.is_err());
    }
}
