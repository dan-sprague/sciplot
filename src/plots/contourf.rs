//! `contourf`: filled bands between levels of a 2D field (Makie's `contourf`).
//!
//! Bands follow Isoband.jl (what Makie uses): band `k` covers `levels[k] <= z < levels[k + 1]`.
//! They are triangulated per grid cell with shared, bit-identical vertices ([`super::marching`]),
//! so the mesh has no seams on the GPU (MSAA) and each band becomes one merged path in SVG.
//!
//! Provenance: adapted from Makie 0.24.14 `src/basic_recipes/contourf.jl` (`_get_isoband_levels`,
//! the `computed_levels` rules of `register_contourf_computations!`, `compute_contourf_colormap`,
//! `compute_lowcolor`/`compute_highcolor`, `_calculate_polys!`; defaults of `@recipe Contourf`);
//! the hover text follows `show_data(::DataInspector, ::Contourf, ...)` in
//! `src/interaction/inspector.jl`. Band semantics: see `marching.rs`. MIT licensed; see
//! THIRD_PARTY_NOTICES.md.

use super::marching::{self, GridField};
use super::{Levels, PlotImpl, PlotKind, add_to_axis, plot_common};
use crate::attrs::{Conv, attributes};
use crate::color::{Color, Colormap, IntoColor, LUT_SIZE};
use crate::data::{CellCoords, CellSpecKind as Spec, Data2D};
use crate::figure::{Dirty, FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::scene::drawlist::{MeshPrim, MeshVertex, Prim};
use crate::transform::Scale;
use parking_lot::Mutex;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// How `contourf` reads explicit `levels` (Makie's `mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ContourfMode {
    /// Levels are z values.
    #[default]
    Normal,
    /// Levels are fractions of the data's range: `0.0` is the minimum, `1.0` the maximum.
    Relative,
}
crate::attrs::conv_identity!(ContourfMode);

/// A band beyond the outermost level (Makie's `extendlow` / `extendhigh`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Extend {
    /// The colormap's end color (Makie's `:auto`).
    Auto,
    /// A fixed color.
    Color(Color),
}

impl Conv<Option<Extend>> for Extend {
    fn conv(self) -> Option<Extend> {
        Some(self)
    }
}
impl Conv<Option<Extend>> for Option<Extend> {
    fn conv(self) -> Option<Extend> {
        self
    }
}
impl<C: IntoColor> Conv<Option<Extend>> for C {
    #[track_caller]
    fn conv(self) -> Option<Extend> {
        Some(Extend::Color(self.into_color()))
    }
}

/// A filled-contour plot handle (Makie's `Contourf`).
///
/// ```no_run
/// use sciplot::prelude::*;
/// let (nx, ny) = (120, 90);
/// let xs = linspace(-3.0, 3.0, nx);
/// let ys = linspace(-2.0, 2.0, ny);
/// let z: Vec<f64> = (0..nx * ny).map(|k| (-(xs[k % nx].powi(2) + ys[k / nx].powi(2))).exp()).collect();
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// let cf = contourf!(ax, &xs, &ys, Field::new(&z, nx, ny); levels = 8);
/// Colorbar::new(fig.at(1, 2), &cf);
/// fig.save("contourf.png").unwrap();
/// ```
#[derive(Clone)]
pub struct Contourf {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

/// Triangles per band, in data space, cached by grid generation and band edges.
#[derive(Debug, Default)]
pub(crate) struct Bands {
    /// Flat triangle list (3 points each).
    pub tris: Vec<[f64; 2]>,
    /// `(band, end)`: triangles `prev_end..end` of `tris` belong to `band`.
    pub ranges: Vec<(usize, usize)>,
    /// Finite bounds of the triangles in data space, `[x0, x1, y0, y1]`.
    pub bounds: Option<[f64; 4]>,
}

type BandCache = Arc<Mutex<Option<(u64, Arc<Bands>)>>>;

#[derive(Clone, Debug)]
pub(crate) struct ContourfState {
    pub field: GridField,
    pub attrs: ContourfAttrs,
    pub cache: BandCache,
}

attributes! {
    Contourf(ContourfAttrs, ContourfResolved, ContourfTheme) via with_attrs {
        /// Number of equal bands covering the data's range (default 10), or explicit ascending
        /// band edges (`n` edges make `n - 1` bands).
        levels: Levels = |_| Levels::Count(10), LIMITS;
        /// How explicit `levels` are read: z values (default) or fractions of the data's range.
        mode: ContourfMode = |_| ContourfMode::Normal, STYLE;
        /// Also fill below the first level: `Extend::Auto` (the colormap's first color) or a color.
        extendlow: Option<Extend> = |_| None, STYLE;
        /// Also fill above the last level: `Extend::Auto` (the colormap's last color) or a color.
        extendhigh: Option<Extend> = |_| None, STYLE;
        /// Colormap sampled once per band (default viridis).
        colormap: Colormap = |_| Colormap::VIRIDIS, STYLE;
        /// Opacity multiplier (an sciplot addition).
        alpha: f64 = |_| 1.0, STYLE;
    }
}

plot_common!(Contourf);
super::color_mapped!(Contourf);

/// Band colors: Makie's `compute_contourf_colormap` (a categorical colormap of one color per
/// band, sampled evenly from the colormap and shifted inwards for automatic extensions) plus the
/// extension colors.
pub(crate) struct BandColors {
    pub bands: Vec<Color>,
    pub low: Option<Color>,
    pub high: Option<Color>,
}

fn band_colors(cmap: &Colormap, nbands: usize, low: Option<Extend>, high: Option<Extend>) -> BandColors {
    let (lo_auto, hi_auto) = (low == Some(Extend::Auto), high == Some(Extend::Auto));
    let nedges = nbands + 1;
    // Makie first takes `m` evenly spaced colors (`cgrad(cmap, m; categorical = true)`), drops
    // the ends reserved for automatic extensions, then samples that list evenly per band.
    let base: Vec<Color> = if !lo_auto && !hi_auto {
        Vec::new()
    } else {
        let m = nedges + lo_auto as usize + hi_auto as usize;
        let all: Vec<Color> = (0..m).map(|k| cmap.sample(k as f64 / (m - 1) as f64)).collect();
        all[lo_auto as usize..m - hi_auto as usize].to_vec()
    };
    let at = |t: f64| -> Color {
        if base.is_empty() {
            return cmap.sample(t);
        }
        let f = t.clamp(0.0, 1.0) * (base.len() - 1) as f64;
        let i = (f.floor() as usize).min(base.len() - 1);
        base[i].lerp(base[(i + 1).min(base.len() - 1)], (f - i as f64) as f32)
    };
    let bands = (0..nbands).map(|k| at(if nbands > 1 { k as f64 / (nbands - 1) as f64 } else { 0.0 })).collect();
    let ext = |e: Option<Extend>, end: f64| match e {
        None => None,
        Some(Extend::Auto) => Some(cmap.sample(end)),
        Some(Extend::Color(c)) => Some(c),
    };
    BandColors { bands, low: ext(low, 0.0), high: ext(high, 1.0) }
}

impl ContourfState {
    /// The band edges in effect (Makie's `computed_levels`), ascending; empty without finite data.
    pub(crate) fn levels(&self, r: &ContourfResolved) -> Vec<f64> {
        let Some((lo, hi)) = self.field.zrange() else { return Vec::new() };
        // Makie computes contourf levels in Float32 (its z matrix is Float32).
        let f32r = |v: f64| v as f32 as f64;
        let (lo32, hi32) = (f32r(lo), f32r(hi));
        let mut v: Vec<f64> = match &r.levels {
            Levels::Count(n) => {
                let n = (*n).max(1);
                let (a, b) = if super::contour::approx_eq(lo, hi) {
                    let d = lo32.abs().max(1.0);
                    (lo32 - d, hi32 + d)
                } else {
                    // Makie's `nextfloat(Float32(max))`: the maximum falls inside the last band.
                    (lo32, (hi as f32).next_up() as f64)
                };
                let mut v: Vec<f64> = (0..=n).map(|k| f32r(a + (b - a) * (k as f64 / n as f64))).collect();
                // Keep the extreme values inside the outer bands despite the rounding.
                v[0] = v[0].min(lo);
                v[n] = v[n].max(hi.next_up());
                v
            }
            Levels::Values(v) => match r.mode {
                ContourfMode::Normal => v.iter().map(|x| f32r(*x)).collect(),
                ContourfMode::Relative => v.iter().map(|t| f32r(t * (hi32 - lo32) + lo32)).collect(),
            },
        };
        v.retain(|x| !x.is_nan());
        if !v.is_sorted() {
            crate::warn_once("contourf: levels must be ascending; sorting them");
            v.sort_by(f64::total_cmp);
        }
        v
    }

    /// Band edges including the extensions (Makie's `_calculate_polys!`).
    fn edges(&self, r: &ContourfResolved) -> Vec<f64> {
        let mut edges = self.levels(r);
        if edges.len() < 2 {
            return Vec::new();
        }
        if r.extendlow.is_some() {
            edges.insert(0, f64::NEG_INFINITY);
        }
        if r.extendhigh.is_some() {
            edges.push(f64::INFINITY);
        }
        edges
    }

    /// Band edges including the extensions, each band's color, and the levels.
    fn bands_and_colors(&self, r: &ContourfResolved) -> Option<(Vec<f64>, Vec<Color>, Vec<f64>)> {
        let levels = self.levels(r);
        if levels.len() < 2 {
            return None;
        }
        let c = band_colors(&r.colormap, levels.len() - 1, r.extendlow, r.extendhigh);
        let mut edges = Vec::with_capacity(levels.len() + 2);
        let mut colors = Vec::with_capacity(levels.len() + 1);
        if let Some(low) = c.low {
            edges.push(f64::NEG_INFINITY);
            colors.push(low);
        }
        edges.extend_from_slice(&levels);
        colors.extend(c.bands);
        if let Some(high) = c.high {
            edges.push(f64::INFINITY);
            colors.push(high);
        }
        let a = r.alpha as f32;
        Some((edges, colors.into_iter().map(|c| c.with_alpha(c.a * a)).collect(), levels))
    }

    /// Triangles of every band (cached). `stacked` fills band `k` as the whole region
    /// `edges[k] <= z < edges[last]`: drawn in order, each band then covers the anti-aliased edge
    /// of the one below instead of leaving a seam of background between them (conflation in
    /// vector renderers). Only for opaque colors; otherwise the bands partition the grid exactly.
    fn bands(&self, edges: &[f64], stacked: bool) -> Arc<Bands> {
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.field.generation, stacked).hash(&mut h);
            edges.iter().for_each(|l| l.to_bits().hash(&mut h));
            h.finish()
        };
        let mut cache = self.cache.lock();
        if let Some((k, v)) = cache.as_ref()
            && *k == key
        {
            return v.clone();
        }
        let g = self.field.grid();
        let mut b = Bands::default();
        let top = edges.last().copied().unwrap_or(f64::NAN);
        for (k, w) in edges.windows(2).enumerate() {
            let hi = if stacked { top } else { w[1] };
            marching::isoband(&g, self.field.level(w[0]), self.field.level(hi), &mut b.tris);
            b.ranges.push((k, b.tris.len()));
        }
        b.bounds = super::point_bounds(&b.tris, Scale::Identity, Scale::Identity);
        let b = Arc::new(b);
        *cache = Some((key, b.clone()));
        b
    }
}

/// Band-center label like Makie's inspector (`@sprintf("%0.3f", level)`).
fn level_text(v: f64) -> String {
    if v.is_infinite() { if v > 0.0 { "Inf".into() } else { "-Inf".into() } } else { format!("{v:.3}") }
}

impl PlotImpl for ContourfState {
    fn cycle_group(&self) -> &'static str {
        "contourf"
    }

    fn color_is_auto(&self, _theme: &crate::theme::Theme) -> bool {
        false
    }

    /// The grid with automatic levels (the bands cover it); with explicit levels, the filled
    /// region (Makie's poly limits). Levels set only in a theme are not seen here.
    fn data_bounds(&self, xs: Scale, ys: Scale) -> Option<[f64; 4]> {
        if self.tight_limits() {
            return self.field.bounds(xs, ys);
        }
        let r = self.attrs.resolve(&ContourfAttrs::default(), &crate::theme::Globals::default());
        let edges = self.edges(&r);
        if edges.is_empty() {
            return None;
        }
        let bands = self.bands(&edges, true);
        let [x0, x1, y0, y1] = bands.bounds?;
        let (a, b, c, d) = (xs.forward(x0), xs.forward(x1), ys.forward(y0), ys.forward(y1));
        if [a, b, c, d].iter().all(|v| v.is_finite()) {
            Some([a.min(b), a.max(b), c.min(d), c.max(d)])
        } else {
            super::point_bounds(&bands.tris, xs, ys)
        }
    }

    /// Makie: an integer `levels` covers the whole grid, so the axis uses tight limits.
    fn tight_limits(&self) -> bool {
        matches!(self.attrs.levels, None | Some(Levels::Count(_)))
    }

    fn legend_elements(&self, _ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        // Makie's poly element of a value-colored mesh: the legend's default patch color.
        vec![super::LegendElement::Poly {
            color: super::legend_elements::DEFAULT_POLYCOLOR,
            strokecolor: Color::TRANSPARENT,
            strokewidth: 0.0,
        }]
    }

    /// Makie's contourf colorbar: one block per band between the first and last level, with
    /// triangles for the extensions.
    fn colormapping(&self, theme: &crate::theme::Theme, g: &crate::theme::Globals) -> Option<super::ResolvedColormap> {
        let r = self.attrs.resolve(&theme.contourf, g);
        let levels = self.levels(&r);
        if levels.len() < 2 {
            return Some(super::ResolvedColormap::unmapped(r.colormap, r.alpha));
        }
        let (lo, hi) = (levels[0], levels[levels.len() - 1]);
        let c = band_colors(&r.colormap, levels.len() - 1, r.extendlow, r.extendhigh);
        // A stepped lookup table: entry i shows the band containing lo + (hi - lo) i / 255.
        let lut: Vec<Color> = (0..LUT_SIZE)
            .map(|i| {
                let v = lo + (hi - lo) * i as f64 / (LUT_SIZE - 1) as f64;
                let k = levels.partition_point(|l| *l <= v).saturating_sub(1).min(c.bands.len() - 1);
                c.bands[k]
            })
            .collect();
        Some(super::ResolvedColormap {
            colormap: Colormap::from_colors(&lut),
            colorrange: (lo, hi),
            lowclip: c.low,
            highclip: c.high,
            alpha: r.alpha,
            mapped: true,
        })
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.contourf, ctx.g);
        let Some((edges, colors, _)) = self.bands_and_colors(&r) else { return };
        let bands = self.bands(&edges, colors.iter().all(|c| c.a >= 1.0));
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (self.field.generation, ctx.conv_key(0)).hash(&mut h);
            edges.iter().for_each(|l| l.to_bits().hash(&mut h));
            colors.iter().for_each(|c| c.to_premul_u32().hash(&mut h));
            h.finish()
        };
        let axis = ctx.axis;
        let verts = ctx.cache.memo(ctx.uid, 0, key, || {
            let (xs, ys) = (axis.attrs.xscale, axis.attrs.yscale);
            let mut v = Vec::with_capacity(bands.tris.len());
            let mut start = 0;
            for &(band, end) in &bands.ranges {
                let color = colors[band].to_premul_u32();
                if colors[band].a > 0.0 {
                    v.extend(bands.tris[start..end].iter().map(|p| {
                        let (sx, sy) = (xs.forward(p[0]), ys.forward(p[1]));
                        let pos =
                            if sx.is_finite() && sy.is_finite() { axis.rebase.to_local(sx, sy) } else { [f32::NAN; 2] };
                        MeshVertex { pos, color }
                    }));
                }
                start = end;
            }
            v
        });
        if !verts.is_empty() {
            let buf = ctx.keyed_buf(0, key, verts);
            ctx.push_data(Prim::Mesh(MeshPrim { verts: buf }));
        }
    }

    /// Makie's contourf inspection: the band under the cursor as `level = <band center>`.
    fn pick(&self, ctx: &mut super::pick::PickCtx<'_>) -> Option<super::pick::Hover> {
        let p = ctx.cursor_data()?;
        let r = self.attrs.resolve(&ctx.theme.contourf, ctx.g);
        let (edges, colors, _) = self.bands_and_colors(&r)?;
        let cell = self.field.cell_at(p)?;
        let g = self.field.grid();
        let k = edges
            .windows(2)
            .position(|w| marching::band_contains(&g, cell, self.field.level(w[0]), self.field.level(w[1]), p))?;
        if colors[k].a <= 0.0 {
            return None;
        }
        Some(super::pick::Hover {
            dist: ctx.radius,
            anchor: ctx.cursor,
            text: format!("level = {}", level_text(0.5 * (edges[k] + edges[k + 1]))),
            ring: None,
            outline: None,
        })
    }
}

impl Contourf {
    fn with_attrs(&self, f: impl FnOnce(&mut ContourfAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Contourf(s) = &mut p.kind {
                f(&mut s.attrs)
            }
        });
    }

    #[track_caller]
    pub(crate) fn create(ax: &crate::Axis, x: Spec, y: Spec, z: impl Data2D) -> Contourf {
        let field = GridField::new(x, y, &z).unwrap_or_else(|e| panic!("contourf {e}"));
        let st = ContourfState { field, attrs: ContourfAttrs::default(), cache: BandCache::default() };
        Contourf { sh: ax.sh.clone(), id: add_to_axis(ax, PlotKind::Contourf(st)) }
    }

    /// Replaces the values (converted on the calling thread). New dimensions recompute the grid
    /// coordinates from the ones the plot was created with (vector coordinates must then match).
    ///
    /// # Panics
    /// If the dimensions changed and explicit coordinate vectors no longer fit.
    #[track_caller]
    pub fn set_data(&self, z: impl Data2D) -> Contourf {
        let dims = z.dims();
        let (values, enc) = marching::encode(&z);
        let res = self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            p.data_rev += 1;
            match &mut p.kind {
                PlotKind::Contourf(s) => s.field.set_values(dims, values, enc),
                _ => Ok(()),
            }
        });
        if let Some(Err(e)) = res {
            panic!("Contourf::set_data {e}");
        }
        self.clone()
    }

    /// Replaces the grid coordinates (same rules as [`Axis::contourf_xy`](crate::Axis::contourf_xy)).
    ///
    /// # Panics
    /// If a coordinate vector's length differs from the number of grid points.
    #[track_caller]
    pub fn set_coords(&self, x: impl CellCoords, y: impl CellCoords) -> Contourf {
        let (xs, ys) = (x.cell_spec().0, y.cell_spec().0);
        let res = self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            p.data_rev += 1;
            match &mut p.kind {
                PlotKind::Contourf(s) => s.field.set_coords(xs, ys),
                _ => Ok(()),
            }
        });
        if let Some(Err(e)) = res {
            panic!("Contourf::set_coords {e}");
        }
        self.clone()
    }

    /// The band edges in effect (Makie's `computed_levels`, without the extensions).
    pub fn resolved_levels(&self) -> Vec<f64> {
        let st = self.sh.state.lock();
        let g = st.theme.globals();
        match st.plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Contourf(s)) => s.levels(&s.attrs.resolve(&st.theme.contourf, &g)),
            _ => Vec::new(),
        }
    }
}

impl crate::Axis {
    /// Makie's `contourf!(ax, z)`: filled bands of `z`, whose point `(i, j)` (0-based) sits at
    /// `(i + 1, j + 1)`.
    #[track_caller]
    pub fn contourf(&self, z: impl Data2D) -> Contourf {
        Contourf::create(self, Spec::Index, Spec::Index, z)
    }

    /// Makie's `contourf!(ax, x, y, z)`: `z[i, j]` is the value at the grid point `(x_i, y_j)`;
    /// `x` and `y` are read as point positions like [`Axis::contour_xy`](crate::Axis::contour_xy).
    #[track_caller]
    pub fn contourf_xy(&self, x: impl CellCoords, y: impl CellCoords, z: impl Data2D) -> Contourf {
        Contourf::create(self, x.cell_spec().0, y.cell_spec().0, z)
    }
}

impl crate::GridPosition {
    /// Makie's `contourf(fig[r, c], z)`: a new Axis at this position with a filled contour plot.
    #[track_caller]
    pub fn contourf(&self, z: impl Data2D) -> Contourf {
        crate::Axis::new(self.clone()).contourf(z)
    }

    /// Makie's `contourf(fig[r, c], x, y, z)`: a new Axis at this position with a filled contour
    /// plot.
    #[track_caller]
    pub fn contourf_xy(&self, x: impl CellCoords, y: impl CellCoords, z: impl Data2D) -> Contourf {
        crate::Axis::new(self.clone()).contourf_xy(x, y, z)
    }
}

/// Makie's `contourf(z)`: a new Figure and Axis with filled bands of `z` (points at `1..=nx`,
/// `1..=ny`). Returns the plot handle; call `.save(..)` or `.show()` on it.
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn contourf(z: impl Data2D) -> Contourf {
    crate::Figure::new().at(1, 1).contourf(z)
}

/// Makie's `contourf(x, y, z)`: a new Figure and Axis with filled bands on the given grid points
/// (see [`Axis::contour_xy`](crate::Axis::contour_xy)).
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn contourf_xy(x: impl CellCoords, y: impl CellCoords, z: impl Data2D) -> Contourf {
    crate::Figure::new().at(1, 1).contourf_xy(x, y, z)
}

#[cfg(test)]
mod tests {
    use super::super::contour::tests::{build, mixture};
    use crate::prelude::*;
    use crate::scene::SceneCache;
    use crate::scene::drawlist::Prim;

    fn mixture_cf() -> Contourf {
        let (xs, ys, z) = mixture();
        contourf_xy(&xs, &ys, Field::new(&z, 120, 100))
    }

    fn assert_close(what: &str, a: &[f64], b: &[f64], tol: f64) {
        assert_eq!(a.len(), b.len(), "{what}: {a:?} vs {b:?}");
        assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() <= tol), "{what}: {a:?} vs {b:?}");
    }

    fn limits(cf: &Contourf) -> Vec<f64> {
        build(&cf.figure()).1[0].limits.to_vec()
    }

    /// Band colors of the plot (as drawn), in order.
    fn colors(cf: &Contourf) -> Vec<[f64; 3]> {
        let st = cf.sh.snapshot();
        let Some(crate::plots::PlotKind::Contourf(s)) = st.plot(cf.id).map(|p| &p.kind) else { unreachable!() };
        let r = s.attrs.resolve(&st.theme.contourf, &st.theme.globals());
        let (_, cs, _) = s.bands_and_colors(&r).unwrap();
        cs.iter().map(|c| [c.r as f64, c.g as f64, c.b as f64]).collect()
    }

    /// Confirmed with CairoMakie (tools/contour_check.jl): Float32 levels, one categorical color
    /// per band, tight limits for an integer `levels`.
    #[test]
    fn levels_colors_and_limits_match_makie() {
        let cf = mixture_cf().levels(8);
        let makie = [
            1.00986156e-7,
            0.12555256,
            0.25110504,
            0.3766575,
            0.50220996,
            0.62776244,
            0.75331485,
            0.8788673,
            1.0044198,
        ];
        assert_close("levels = 8", &cf.resolved_levels(), &makie.map(|v: f64| v as f32 as f64), 1e-7);
        let viridis = [
            [0.26700401306152344, 0.004873999860137701, 0.3294149935245514],
            [0.27473542945725576, 0.19696899609906332, 0.49725042496408733],
            [0.212666854262352, 0.35910243647439133, 0.5516348651477269],
            [0.1529508573668344, 0.4980528567518506, 0.5576848643166679],
            [0.12204571600471223, 0.6321070109094892, 0.5308480092457363],
            [0.29000071116856163, 0.7588464277131216, 0.42782557010650635],
            [0.6221707122666493, 0.8538152916090829, 0.22622399670737142],
            [0.9932479858398438, 0.9061570167541504, 0.14393599331378937],
        ];
        assert_close("viridis bands", &colors(&cf).concat(), &viridis.concat(), 2e-6);
        assert_close("tight limits", &limits(&cf), &[-3.0, 3.0, -2.5, 2.5], 1e-12);

        // Automatic extensions shift the band colors inwards.
        let levels = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
        let cf = mixture_cf().levels(levels).extendlow(Extend::Auto).extendhigh(Extend::Auto).colormap("plasma");
        let plasma = [
            [0.3278397193976811, 0.0066347141484064745, 0.6402851428304399],
            [0.5901820021016256, 0.07814228787486038, 0.6199139824935368],
            [0.791743129491806, 0.2783595781241144, 0.4729085798774447],
            [0.9315254028354372, 0.4901962748595646, 0.3157263270446233],
            [0.9945232783045088, 0.7382704274994986, 0.16745057063443325],
        ];
        let cs = colors(&cf);
        assert_eq!(cs.len(), 7, "5 bands and 2 extensions");
        assert_close("plasma bands", &cs[1..6].concat(), &plasma.concat(), 2e-6);
        let first = Colormap::PLASMA.first();
        assert_close("extendlow", &cs[0], &[first.r as f64, first.g as f64, first.b as f64], 0.0);
        assert_close("explicit levels, extended: margins", &limits(&cf), &[-3.3, 3.3, -2.75, 2.75], 1e-6);
        let m = cf.colormapping().unwrap();
        assert_eq!(m.colorrange, (0.1f32 as f64, 0.6f32 as f64));
        assert_eq!((m.lowclip, m.highclip), (Some(Colormap::PLASMA.first()), Some(Colormap::PLASMA.last())));
        // The colorbar's stepped colormap: band 3 (0.35) is the third band color.
        let c = m.colormap.sample((0.35 - 0.1) / 0.5);
        assert_close("colorbar step", &[c.r as f64, c.g as f64, c.b as f64], &plasma[2], 2e-6);
    }

    /// Explicit levels: limits are those of the filled region (Makie's poly limits), and
    /// `mode = relative` reads the levels as fractions of the range.
    #[test]
    fn explicit_levels_use_the_filled_region() {
        let cf = mixture_cf().levels([0.3, 0.6]);
        let makie = [-2.1438640626161423, 2.5332078011720505, -1.8593439963459888, 2.1006441023945728];
        assert_close("band limits", &limits(&cf), &makie, 1e-5);
        assert!(!cf.colormapping().unwrap().highclip.is_some());

        let cf = mixture_cf().levels([0.1, 0.3, 0.5, 0.7]).mode(ContourfMode::Relative).extendhigh(RED);
        assert_close("relative levels", &cf.resolved_levels(), &[0.10044206, 0.30132598, 0.5022099, 0.70309377], 1e-7);
        let makie = [-2.5425339260078275, 3.0860639133430325, -2.1818436655346076, 2.6498726877514045];
        assert_close("relative limits", &limits(&cf), &makie, 1e-5);
        assert_eq!(cf.colormapping().unwrap().highclip, Some(RED));
        assert_eq!(colors(&cf).last(), Some(&[1.0, 0.0, 0.0]));
    }

    /// Opaque bands are stacked (each covers the rest of the range, drawn in order); translucent
    /// ones partition the grid. Either way, one mesh.
    #[test]
    fn mesh_is_stacked_when_opaque() {
        let cf = mixture_cf().levels(4);
        let tris = |cf: &Contourf| -> usize {
            let (dl, _) = build(&cf.figure());
            dl.items
                .iter()
                .map(|i| match &i.prim {
                    Prim::Mesh(m) if i.space != crate::scene::drawlist::Space::Figure => m.verts.len() / 3,
                    _ => 0,
                })
                .sum()
        };
        let opaque = tris(&cf);
        let translucent = tris(&cf.alpha(0.5));
        assert!(opaque > 0 && translucent > 0);
        let (dl, _) = build(&cf.figure());
        let meshes = dl.items.iter().filter(|i| matches!(i.prim, Prim::Mesh(_))).count();
        assert_eq!(meshes, 1);
    }

    #[test]
    fn constant_data_and_set_data() {
        let cf = contourf(Field::new(&[2.0; 4], 2, 2)).levels(2);
        // Makie widens constant data by max(1, |z|): levels 0, 2, 4.
        assert_close("constant", &cf.resolved_levels(), &[0.0, 2.0, 4.0], 1e-6);
        cf.set_data(Field::new(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0], 3, 2));
        let lv = cf.resolved_levels();
        assert_eq!(lv.len(), 3);
        assert!(lv[0] <= 0.0 && lv[2] > 5.0);
        assert!(contourf(Field::new(&[f64::NAN; 4], 2, 2)).resolved_levels().is_empty());
    }

    #[test]
    fn pick_reports_the_band_center() {
        let n = 11;
        let xs = linspace(0.0, 1.0, n);
        let z: Vec<f64> = (0..n * n).map(|k| xs[k % n]).collect();
        let cf = contourf_xy(&xs, &xs, Field::new(&z, n, n)).levels([0.0, 0.25, 0.5, 1.0]);
        let st = cf.sh.snapshot();
        let (_, axes) = crate::scene::build(&st, None, &mut SceneCache::new());
        let a = &axes[0];
        let g = st.theme.globals();
        let p = st.plots[0].as_ref().unwrap();
        let mut cache = crate::plots::pick::PickCache::default();
        let mut at = |x: f64| {
            let mut ctx = crate::plots::pick::PickCtx {
                axis: a,
                cursor: a.to_units(x, 0.5).unwrap(),
                radius: 10.0,
                uid: p.uid,
                data_rev: p.data_rev,
                theme: &st.theme,
                g: &g,
                cache: &mut cache,
            };
            p.kind.imp().pick(&mut ctx).map(|h| h.text)
        };
        assert_eq!(at(0.3).as_deref(), Some("level = 0.375"));
        assert_eq!(at(0.9).as_deref(), Some("level = 0.750"));
        assert_eq!(at(0.1).as_deref(), Some("level = 0.125"));
    }
}
