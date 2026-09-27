//! `surface`: a height field `z[i, j]` over the grid `(x_i, y_j)` in an [`Axis3`](crate::Axis3)
//! (Makie's `Surface`).
//!
//! Colored by `z` through the colormap by default, and shaded like Makie's default
//! (`shading = true`: `FastShading`, an ambient light plus one directional light that follows the
//! camera, Blinn-Phong with `diffuse = 1`, `specular = 0.2`, `shininess = 32`); `shading(false)`
//! draws the flat colormap colors. The GPU lights per pixel, the SVG backend per triangle.
//! Surfaces write depth, so lines and markers behind them are hidden.

use super::lines3d::{add_to_axis3, plot3d_common};
use super::{PlotImpl, PlotKind};
use crate::attrs::attributes;
use crate::color::{Color, Colormap, MappingAttrs, ValueEncoding};
use crate::data::{Data1D, Data2D};
use crate::figure::{FigShared, PlotId};
use crate::scene::axis3::{Plot3dCtx, Plot3dImpl};
use crate::scene::drawlist::{Buf, BufKey, Material, Mesh3dPrim, Prim, PrimColor, Vertex3d};
use std::sync::Arc;

/// A surface plot handle (Makie's `surface!(ax3, x, y, z)`).
///
/// ```no_run
/// use sciplot::prelude::*;
/// let (nx, ny) = (40, 30);
/// let x = linspace(-2.0, 2.0, nx);
/// let y = linspace(-1.5, 1.5, ny);
/// let z: Vec<f64> = (0..nx * ny).map(|k| (x[k % nx] * y[k / nx]).sin()).collect();
/// let fig = Figure::new();
/// let s = Axis3::new(fig.at(1, 1)).surface(&x, &y, Field::new(&z, nx, ny));
/// Colorbar::new(fig.at(1, 2), &s);
/// fig.save("surface.png").unwrap();
/// ```
#[derive(Clone)]
pub struct Surface {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct SurfaceState {
    pub x: Arc<Vec<f64>>,
    pub y: Arc<Vec<f64>>,
    /// `nx * ny` heights, x fastest.
    pub z: Arc<Vec<f64>>,
    pub nx: usize,
    pub ny: usize,
    pub enc: ValueEncoding,
    pub attrs: SurfaceAttrs,
}

attributes! {
    Surface(SurfaceAttrs, SurfaceResolved, SurfaceTheme) via with_attrs {
        /// One color for the whole surface (default `None`: `z` through the colormap).
        color: Option<Color> = |_| None, STYLE;
        /// Colormap for the heights (default viridis).
        colormap: Colormap = |_| Colormap::VIRIDIS, STYLE;
        /// `(lo, hi)` mapped to the colormap ends; default: the finite extrema of `z`.
        colorrange: Option<[f64; 2]> = |_| None, STYLE;
        /// Color for heights below the colorrange (default: the first colormap color).
        lowclip: Option<Color> = |_| None, STYLE;
        /// Color for heights above the colorrange (default: the last colormap color).
        highclip: Option<Color> = |_| None, STYLE;
        /// Color for NaN heights (their cells are left out; default transparent).
        nan_color: Color = |_| Color::TRANSPARENT, STYLE;
        /// Opacity multiplier.
        alpha: f64 = |_| 1.0, STYLE;
        /// Makie's `shading`: lit by the axis' lights (default true).
        shading: bool = |_| true, STYLE;
        /// How strongly the surface scatters the directional light (Makie default 1).
        diffuse: f64 = |_| 1.0, STYLE;
        /// Strength of the specular highlight (Makie default 0.2).
        specular: f64 = |_| 0.2, STYLE;
        /// Sharpness of the specular highlight (Makie default 32).
        shininess: f64 = |_| 32.0, STYLE;
    }
}

plot3d_common!(Surface);
super::color_mapped!(Surface);

impl SurfaceResolved {
    fn mapping(&self) -> MappingAttrs<'_> {
        MappingAttrs {
            colormap: &self.colormap,
            colorrange: self.colorrange,
            lowclip: self.lowclip,
            highclip: self.highclip,
            nan_color: self.nan_color,
            alpha: self.alpha,
        }
    }
}

impl SurfaceState {
    #[track_caller]
    fn new(x: Vec<f64>, y: Vec<f64>, z: impl Data2D) -> SurfaceState {
        let (nx, ny) = z.dims();
        assert!(
            x.len() == nx && y.len() == ny,
            "surface: z is {nx} × {ny} but x has {} and y {} values (one per row/column of z)",
            x.len(),
            y.len()
        );
        let enc = ValueEncoding::new(z.extrema());
        let mut v = Vec::new();
        z.write_f32(&mut v, enc.off, enc.k);
        // Heights as f64 again (relative precision of f32 over the value range).
        let zs = v.iter().map(|v| *v as f64 / enc.k + enc.off).collect();
        SurfaceState { x: Arc::new(x), y: Arc::new(y), z: Arc::new(zs), nx, ny, enc, attrs: SurfaceAttrs::default() }
    }

    fn point(&self, i: usize, j: usize) -> [f64; 3] {
        [self.x[i], self.y[j], self.z[j * self.nx + i]]
    }

    /// Makie's `nan_aware_normals` of the grid's quads (data space, normalized).
    fn normals(&self) -> Vec<[f64; 3]> {
        let (nx, ny) = (self.nx, self.ny);
        let mut n = vec![[0.0f64; 3]; nx * ny];
        let finite = |p: [f64; 3]| p.iter().all(|v| v.is_finite());
        for j in 0..ny.saturating_sub(1) {
            for i in 0..nx.saturating_sub(1) {
                let (a, b, c) = (self.point(i, j), self.point(i + 1, j), self.point(i + 1, j + 1));
                if !(finite(a) && finite(b) && finite(c)) {
                    continue;
                }
                let (u, w) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
                let cr = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                for k in [j * nx + i, j * nx + i + 1, (j + 1) * nx + i + 1, (j + 1) * nx + i] {
                    for d in 0..3 {
                        n[k][d] += cr[d];
                    }
                }
            }
        }
        for v in &mut n {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if l > 0.0 {
                *v = v.map(|c| c / l);
            }
        }
        n
    }

    /// Grid indices of the triangle list: quads without NaN corners, split `(a, b, c), (a, c, d)`.
    fn triangles(&self) -> Vec<usize> {
        let (nx, ny) = (self.nx, self.ny);
        let ok = |k: usize| self.z[k].is_finite() && self.x[k % nx].is_finite() && self.y[k / nx].is_finite();
        let mut out = Vec::new();
        for j in 0..ny.saturating_sub(1) {
            for i in 0..nx.saturating_sub(1) {
                let q = [j * nx + i, j * nx + i + 1, (j + 1) * nx + i + 1, (j + 1) * nx + i];
                if q.iter().all(|k| ok(*k)) {
                    out.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
                }
            }
        }
        out
    }
}

impl PlotImpl for SurfaceState {
    fn cycle_group(&self) -> &'static str {
        "surface"
    }

    fn color_is_auto(&self, _theme: &crate::theme::Theme) -> bool {
        false
    }

    /// Not a 2D plot: it lives in an Axis3.
    fn data_bounds(&self, _: crate::transform::Scale, _: crate::transform::Scale) -> Option<[f64; 4]> {
        None
    }

    fn emit(&self, _ctx: &mut crate::scene::PlotCtx<'_>) {}

    fn colormapping(&self, theme: &crate::theme::Theme, g: &crate::theme::Globals) -> Option<super::ResolvedColormap> {
        let r = self.attrs.resolve(&theme.surface, g);
        if r.color.is_some() {
            return Some(super::ResolvedColormap::unmapped(r.colormap, r.alpha));
        }
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
}

impl Plot3dImpl for SurfaceState {
    fn bounds(&self) -> Option<[f64; 6]> {
        let (x, y, z) = (
            crate::data::finite_extrema(self.x.iter().copied())?,
            crate::data::finite_extrema(self.y.iter().copied())?,
            self.enc.extrema?,
        );
        Some([x.0, x.1, y.0, y.1, z.0, z.1])
    }

    fn is_mesh(&self) -> bool {
        true
    }

    fn emit3(&self, ctx: &mut Plot3dCtx<'_>) {
        if self.nx < 2 || self.ny < 2 {
            return;
        }
        let r = self.attrs.resolve(&ctx.theme.surface, ctx.g);
        let key = ctx.conv_key(0);
        let rb = ctx.frame.rebase;
        let tris = ctx.cache.memo(ctx.uid, 2, ctx.data_rev, || self.triangles());
        let verts = ctx.cache.memo(ctx.uid, 0, key, || {
            let normals = self.normals();
            tris.iter()
                .map(|&k| {
                    let (i, j) = (k % self.nx, k / self.nx);
                    let nm = normals[k];
                    Vertex3d { pos: rb.to_local(self.point(i, j)), normal: nm.map(|c| c as f32) }
                })
                .collect()
        });
        if verts.is_empty() {
            return;
        }
        let verts = Buf { key: Some(BufKey { uid: ctx.uid, part: 0, rev: key }), data: verts };
        let a = r.alpha as f32;
        let color = match r.color {
            Some(c) => PrimColor::Uniform(c.with_alpha(c.a * a)),
            None => {
                let values = ctx.cache.memo(ctx.uid, 1, ctx.data_rev, || {
                    tris.iter().map(|&k| self.enc.encode(self.z[k])).collect::<Vec<f32>>()
                });
                PrimColor::Values(ctx.data_buf(1, values), r.mapping().mapping(&self.enc))
            }
        };
        let shading = r.shading.then_some(Material {
            diffuse: r.diffuse as f32,
            specular: r.specular as f32,
            shininess: r.shininess as f32,
        });
        let view = ctx.view.clone();
        ctx.push(Prim::Mesh3d(Mesh3dPrim { view, verts, color, shading }));
    }
}

impl Surface {
    fn with_attrs(&self, f: impl FnOnce(&mut SurfaceAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Surface(s) = &mut p.kind {
                f(&mut s.attrs);
            }
        });
    }

    /// Replaces the heights (same grid size, or a new grid with matching `x`/`y`).
    #[track_caller]
    pub fn set_data(&self, x: impl Data1D, y: impl Data1D, z: impl Data2D) -> Self {
        let mut s = SurfaceState::new(x.to_vec_f64(), y.to_vec_f64(), z);
        self.with_slot(crate::figure::Dirty::DATA | crate::figure::Dirty::LIMITS, |p| {
            if let PlotKind::Surface(old) = &mut p.kind {
                s.attrs = std::mem::take(&mut old.attrs);
                *old = s;
                p.data_rev += 1;
            }
        });
        self.clone()
    }
}

impl crate::Axis3 {
    /// Makie's `surface!(ax3, x, y, z)`: `z` is `x.len() × y.len()` (`z[i, j]` at `(x_i, y_j)`;
    /// flat data is x-fastest, see [`Field`](crate::Field)).
    #[track_caller]
    pub fn surface(&self, x: impl Data1D, y: impl Data1D, z: impl Data2D) -> Surface {
        let st = SurfaceState::new(x.to_vec_f64(), y.to_vec_f64(), z);
        let id = add_to_axis3(self, PlotKind::Surface(st));
        Surface { sh: self.sh.clone(), id }
    }
}

impl crate::GridPosition {
    /// Makie's `surface(fig[r, c], x, y, z)`: a new Axis3 at this position with a surface.
    #[track_caller]
    pub fn surface(&self, x: impl Data1D, y: impl Data1D, z: impl Data2D) -> Surface {
        crate::Axis3::new(self.clone()).surface(x, y, z)
    }
}

/// Makie's `surface(x, y, z)`: a new Figure and Axis3 with a surface.
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn surface(x: impl Data1D, y: impl Data1D, z: impl Data2D) -> Surface {
    crate::Figure::new().at(1, 1).surface(x, y, z)
}
