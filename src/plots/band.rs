//! `band`: the filled region between two curves (confidence intervals).
//!
//! Provenance: mesh and stroke adapted from Makie 0.24.14 `src/basic_recipes/band.jl`
//! (`plot!(::Band)`: `band_connect` triangles, NaN handling, `merged_points` stroke); defaults
//! follow its `@recipe Band` and the legend patch `legendelements(::Band)` in
//! `src/makielayout/blocks/legend.jl`. MIT licensed; see THIRD_PARTY_NOTICES.md.

use super::{ColorSpec, PlotImpl, PlotKind, add_to_axis, is_auto, plot_common, point_bounds};
use crate::attrs::attributes;
use crate::color::Color;
use crate::data::Data1D;
use crate::figure::{Dirty, FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::scene::drawlist::{LinesPrim, MeshPrim, MeshVertex, Prim, PrimColor};
use crate::style::{Direction, JoinStyle, LineCap};
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
        /// Opacity multiplier for fill and stroke.
        alpha: f64 = |_| 1.0, STYLE;
        /// Width in units of the lines along the lower and upper curves (Makie default 0).
        strokewidth: f64 = |_| 0.0, STYLE;
        /// Color of the lines along the lower and upper curves (Makie default black).
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

    /// The stroke's polyline: the lower curve, a NaN break, then the upper curve (Makie's
    /// `merged_points`; only points with a NaN coordinate break a curve).
    fn stroke_points(&self, dir: Direction) -> Vec<[f64; 2]> {
        let mk = |x: f64, y: f64| if dir == Direction::X { [x, y] } else { [y, x] };
        let lower = self.x.iter().zip(self.lo.iter()).map(|(x, y)| mk(*x, *y));
        let upper = self.x.iter().zip(self.hi.iter()).map(|(x, y)| mk(*x, *y));
        lower.chain(std::iter::once([f64::NAN; 2])).chain(upper).collect()
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

    fn legend_elements(&self, ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        // Makie draws a band's patch without a stroke.
        let r = self.attrs.resolve(&ctx.theme.band, ctx.g);
        let color = ctx.color(&r.color, true, r.alpha, super::legend_elements::DEFAULT_POLYCOLOR);
        vec![super::LegendElement::Poly { color, strokecolor: Color::TRANSPARENT, strokewidth: 0.0 }]
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

        // Makie's band stroke: `lines!` through the lower curve, a NaN break, then the upper one.
        let sc = r.strokecolor;
        if r.strokewidth > 0.0 && sc.a > 0.0 {
            let pts = ctx.local_points(3, &self.stroke_points(r.direction));
            ctx.push_data(Prim::Lines(LinesPrim {
                pts,
                color: PrimColor::Uniform(sc.with_alpha(sc.a * alpha)),
                width: r.strokewidth as f32,
                pattern: None,
                cap: LineCap::Butt,
                join: JoinStyle::Miter,
                miter_limit: std::f32::consts::FRAC_PI_3,
                segments: false,
                closed: false,
                append: false,
            }));
        }
    }
}

impl Band {
    fn with_attrs(&self, f: impl FnOnce(&mut BandAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Band(s) = &mut p.kind {
                f(&mut s.attrs);
                // `direction` changes the converted points, which are cached per data revision.
                p.data_rev += 1;
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

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::scene::drawlist::{LinesPrim, Prim, PrimColor};
    use crate::scene::{SceneCache, build};

    fn stroke(b: &Band) -> Option<LinesPrim> {
        let dl = build(&b.figure().sh.snapshot(), None, &mut SceneCache::new()).0;
        dl.items.into_iter().find_map(|i| match i.prim {
            Prim::Lines(l) => Some(l),
            _ => None,
        })
    }

    #[test]
    fn stroke_follows_both_curves() {
        let b = band([0.0, 1.0, 2.0], [0.0, f64::NAN, 0.0], [1.0, 2.0, 1.0]);
        assert!(stroke(&b).is_none(), "no stroke by default (strokewidth 0)");
        b.strokewidth(2).strokecolor(RED).alpha(0.5);
        let l = stroke(&b).expect("a stroke");
        // lower (3 points), a NaN break, upper (3 points); a NaN in one curve only breaks it.
        let p = &l.pts.data;
        assert_eq!(p.len(), 7);
        assert!(p[1][1].is_nan() && p[3][0].is_nan() && p[4..].iter().all(|q| q[1].is_finite()));
        assert!(!l.closed && l.width == 2.0);
        assert!(matches!(l.color, PrimColor::Uniform(c) if c.r == 1.0 && (c.a - 0.5).abs() < 1e-6));
        // Changing the direction re-converts the cached points.
        b.direction(Direction::Y);
        let q = stroke(&b).unwrap().pts.data;
        assert!(q[4] != p[4]);
    }
}
