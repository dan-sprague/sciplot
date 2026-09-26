//! `band`: the filled region between two curves (confidence intervals).

use super::{ColorSpec, PlotImpl, PlotKind, add_to_axis, is_auto, plot_common, point_bounds};
use crate::attrs::attributes;
use crate::color::Color;
use crate::data::Data1D;
use crate::figure::{Dirty, FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::scene::drawlist::{MeshPrim, MeshVertex, Prim};
use crate::style::Direction;
use crate::transform::Scale;
use std::sync::Arc;

/// A band handle (Makie's `Band`).
#[derive(Clone)]
pub struct Band {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct BandState {
    pub x: Arc<Vec<f64>>,
    pub lo: Arc<Vec<f64>>,
    pub hi: Arc<Vec<f64>>,
    pub attrs: BandAttrs,
}

attributes! {
    Band(BandAttrs, BandResolved, BandTheme) via with_attrs {
        /// Fill color; defaults to the cycled patch color.
        color: ColorSpec = |_| ColorSpec::Auto, STYLE;
        /// `Direction::X` (default: bands between two y curves over x) or `Direction::Y`.
        direction: Direction = |_| Direction::X, LIMITS;
        alpha: f64 = |_| 1.0, STYLE;
        strokewidth: f64 = |_| 0.0, STYLE;
        strokecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
    }
}

plot_common!(Band);

impl BandState {
    fn points(&self, dir: Direction) -> (Vec<[f64; 2]>, Vec<[f64; 2]>) {
        let n = self.x.len().min(self.lo.len()).min(self.hi.len());
        let mk = |x: f64, y: f64| if dir == Direction::X { [x, y] } else { [y, x] };
        let mut lower = Vec::with_capacity(n);
        let mut upper = Vec::with_capacity(n);
        for i in 0..n {
            let (x, a, b) = (self.x[i], self.lo[i], self.hi[i]);
            if x.is_nan() || a.is_nan() || b.is_nan() {
                lower.push([f64::NAN; 2]);
                upper.push([f64::NAN; 2]);
            } else {
                lower.push(mk(x, a));
                upper.push(mk(x, b));
            }
        }
        (lower, upper)
    }
}

impl PlotImpl for BandState {
    fn cycle_group(&self) -> &'static str {
        "band"
    }

    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool {
        is_auto(self.attrs.color.as_ref(), theme.band.color.as_ref())
    }

    fn data_bounds(&self, xs: Scale, ys: Scale) -> Option<[f64; 4]> {
        let dir = self.attrs.direction.unwrap_or(Direction::X);
        let (mut l, u) = self.points(dir);
        l.extend(u);
        point_bounds(&l, xs, ys)
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.band, ctx.g);
        let alpha = r.alpha as f32;
        let c = ctx.solid_color(&r.color, true).unwrap_or(ctx.g.patchpalette[0]);
        let color = c.with_alpha(c.a * alpha).to_premul_u32();
        let (lower, upper) = self.points(r.direction);
        let lo = ctx.local_points(0, &lower);
        let hi = ctx.local_points(1, &upper);
        let key = ctx.conv_key(2) ^ color as u64;
        let verts = ctx.cache.memo(ctx.uid, 2, key, || {
            let mut v = Vec::with_capacity(lo.data.len() * 6);
            let ok = |p: [f32; 2]| p[0].is_finite() && p[1].is_finite();
            for i in 0..lo.data.len().saturating_sub(1) {
                let (a, b, c2, d) = (lo.data[i], lo.data[i + 1], hi.data[i], hi.data[i + 1]);
                if !(ok(a) && ok(b) && ok(c2) && ok(d)) {
                    continue;
                }
                let m = |p: [f32; 2]| MeshVertex { pos: p, color };
                v.extend_from_slice(&[m(a), m(b), m(c2), m(b), m(d), m(c2)]);
            }
            v
        });
        let buf = ctx.keyed_buf(2, key, verts);
        ctx.push_data(Prim::Mesh(MeshPrim { verts: buf }));
    }
}

impl Band {
    fn with_attrs(&self, f: impl FnOnce(&mut BandAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Band(s) = &mut p.kind {
                f(&mut s.attrs)
            }
        });
    }

    /// Replaces the curves (all three must have equal length).
    #[track_caller]
    pub fn set_data(&self, x: impl Data1D, lo: impl Data1D, hi: impl Data1D) -> Band {
        let (x, lo, hi) = (x.to_vec_f64(), lo.to_vec_f64(), hi.to_vec_f64());
        assert!(x.len() == lo.len() && x.len() == hi.len(), "Band::set_data: lengths differ");
        self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            if let PlotKind::Band(s) = &mut p.kind {
                s.x = Arc::new(x);
                s.lo = Arc::new(lo);
                s.hi = Arc::new(hi);
                p.data_rev += 1;
            }
        });
        self.clone()
    }
}

impl crate::Axis {
    /// Makie's `band!(ax, x, lower, upper)`.
    #[track_caller]
    pub fn band(&self, x: impl Data1D, lo: impl Data1D, hi: impl Data1D) -> Band {
        let (x, lo, hi) = (x.to_vec_f64(), lo.to_vec_f64(), hi.to_vec_f64());
        assert!(
            x.len() == lo.len() && x.len() == hi.len(),
            "band: x, lower and upper have lengths {}, {}, {}",
            x.len(),
            lo.len(),
            hi.len()
        );
        let st = BandState { x: Arc::new(x), lo: Arc::new(lo), hi: Arc::new(hi), attrs: BandAttrs::default() };
        Band { sh: self.sh.clone(), id: add_to_axis(self, PlotKind::Band(st)) }
    }
}

impl crate::GridPosition {
    /// Makie's `band(fig[r, c], x, lower, upper)`.
    #[track_caller]
    pub fn band(&self, x: impl Data1D, lo: impl Data1D, hi: impl Data1D) -> Band {
        crate::Axis::new(self.clone()).band(x, lo, hi)
    }
}

/// Makie's `band(x, lower, upper)`: a new Figure and Axis with a band.
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn band(x: impl Data1D, lo: impl Data1D, hi: impl Data1D) -> Band {
    crate::Figure::new().at(1, 1).band(x, lo, hi)
}
