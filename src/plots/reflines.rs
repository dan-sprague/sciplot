//! `hlines`, `vlines` and `ablines`: reference lines spanning the axis (Makie's `HLines`,
//! `VLines`, `ABLines`).
//!
//! - `hlines(ys)` spans `xmin..xmax` (fractions of the visible x range, default the whole axis)
//!   and counts toward the y autolimits only;
//! - `vlines(xs)` likewise with `ymin..ymax`, counting toward the x autolimits only;
//! - `ablines(intercepts, slopes)` draws `y = a + b·x` across the visible x range and does not
//!   affect the limits (linear axes only, like Makie).
//!
//! All three are drawn as line segments recomputed from the visible limits every frame, and cycle
//! the line palette with their own counters, like Makie.

use super::{ColorSpec, PlotImpl, PlotKind, add_to_axis, is_auto, plot_common};
use crate::attrs::attributes;
use crate::data::Data1D;
use crate::figure::{FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::scene::drawlist::{LinesPrim, Prim};
use crate::style::{JoinStyle, LineCap, Linestyle};
use crate::transform::Scale;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// A handle to `hlines`, `vlines` or `ablines` (Makie's `HLines` / `VLines` / `ABLines`).
///
/// ```no_run
/// use ezviz::prelude::*;
/// let fig = Figure::new();
/// let ax = Axis::new(fig.at(1, 1));
/// ax.lines([0.0, 10.0], [-1.0, 1.0]);
/// hlines!(ax, [0.0]; color = GRAY, linestyle = Linestyle::Dash);
/// vlines!(ax, [2.0, 4.0]; ymin = 0.1, ymax = 0.9);
/// ablines!(ax, 0.0, 0.2);
/// fig.save("reflines.png").unwrap();
/// ```
#[derive(Clone)]
pub struct RefLines {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

/// Makie's `HLines` (the same handle type serves all reference lines).
pub type HLines = RefLines;
/// Makie's `VLines`.
pub type VLines = RefLines;
/// Makie's `ABLines`.
pub type ABLines = RefLines;

/// Which reference lines.
#[derive(Clone, Debug)]
pub(crate) enum RefKind {
    /// Horizontal lines at these y values.
    H(Arc<Vec<f64>>),
    /// Vertical lines at these x values.
    V(Arc<Vec<f64>>),
    /// `(intercept, slope)` pairs.
    AB(Arc<Vec<[f64; 2]>>),
}

#[derive(Clone, Debug)]
pub(crate) struct RefLinesState {
    pub kind: RefKind,
    pub attrs: RefLinesAttrs,
    pub style_rev: u64,
}

attributes! {
    RefLines(RefLinesAttrs, RefLinesResolved, RefLinesTheme) via with_attrs {
        /// A color, `Cycled(i)`, or one color per line.
        color: ColorSpec = |_| ColorSpec::Auto, STYLE;
        /// Line width in units (Makie default 1.5).
        linewidth: f64 = |g| g.linewidth, STYLE;
        /// Dash pattern (`Solid` is Makie's `nothing`).
        linestyle: Linestyle = |_| Linestyle::Solid, STYLE;
        /// Shape of the line ends.
        linecap: LineCap = |_| LineCap::Butt, STYLE;
        /// Opacity multiplier.
        alpha: f64 = |_| 1.0, STYLE;
        /// `hlines`: start of the lines as a fraction of the visible x range.
        xmin: f64 = |_| 0.0, STYLE;
        /// `hlines`: end of the lines as a fraction of the visible x range.
        xmax: f64 = |_| 1.0, STYLE;
        /// `vlines`: start of the lines as a fraction of the visible y range.
        ymin: f64 = |_| 0.0, STYLE;
        /// `vlines`: end of the lines as a fraction of the visible y range.
        ymax: f64 = |_| 1.0, STYLE;
    }
}

plot_common!(RefLines);

fn extrema(v: &[f64], s: Scale) -> Option<(f64, f64)> {
    crate::data::finite_extrema(v.iter().map(|x| s.forward(*x)))
}

impl RefLinesState {
    /// Segment end points in scaled space for the visible range `view` (`[x0, x1, y0, y1]`).
    fn segments(&self, r: &RefLinesResolved, view: [f64; 4], xs: Scale, ys: Scale) -> Vec<[f64; 2]> {
        let [x0, x1, y0, y1] = view;
        let mut out = Vec::new();
        match &self.kind {
            RefKind::H(v) => {
                let (a, b) = (x0 + (x1 - x0) * r.xmin, x0 + (x1 - x0) * r.xmax);
                for y in v.iter() {
                    let y = ys.forward(*y);
                    out.extend([[a, y], [b, y]]);
                }
            }
            RefKind::V(v) => {
                let (a, b) = (y0 + (y1 - y0) * r.ymin, y0 + (y1 - y0) * r.ymax);
                for x in v.iter() {
                    let x = xs.forward(*x);
                    out.extend([[x, a], [x, b]]);
                }
            }
            RefKind::AB(v) => {
                if xs != Scale::Identity || ys != Scale::Identity {
                    crate::warn_once("ablines is only defined for linear axes (like Makie); not drawn");
                    return out;
                }
                for &[a, b] in v.iter() {
                    out.extend([[x0, a + b * x0], [x1, a + b * x1]]);
                }
            }
        }
        out
    }

    fn count(&self) -> usize {
        match &self.kind {
            RefKind::H(v) | RefKind::V(v) => v.len(),
            RefKind::AB(v) => v.len(),
        }
    }
}

impl PlotImpl for RefLinesState {
    fn cycle_group(&self) -> &'static str {
        match self.kind {
            RefKind::H(_) => "hlines",
            RefKind::V(_) => "vlines",
            RefKind::AB(_) => "ablines",
        }
    }

    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool {
        is_auto(self.attrs.color.as_ref(), theme.reflines.color.as_ref())
    }

    fn data_bounds(&self, xs: Scale, ys: Scale) -> Option<[f64; 4]> {
        match &self.kind {
            RefKind::H(v) => extrema(v, ys).map(|(a, b)| [f64::NAN, f64::NAN, a, b]),
            RefKind::V(v) => extrema(v, xs).map(|(a, b)| [a, b, f64::NAN, f64::NAN]),
            RefKind::AB(_) => None,
        }
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.reflines, ctx.g);
        let n = self.count();
        if n == 0 {
            return;
        }
        let a = ctx.axis;
        let (xs, ys) = (a.attrs.xscale, a.attrs.yscale);
        let seg = self.segments(&r, a.view, xs, ys);
        if seg.is_empty() {
            return;
        }
        let local: Vec<[f32; 2]> = seg
            .iter()
            .map(|p| if p[0].is_finite() && p[1].is_finite() { a.rebase.to_local(p[0], p[1]) } else { [f32::NAN; 2] })
            .collect();
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (ctx.data_rev, self.style_rev, a.rebase.epoch, a.view.map(f64::to_bits)).hash(&mut h);
            h.finish()
        };
        let pts = ctx.keyed_buf(0, key, Arc::new(local));
        // One color per line -> one per segment end point.
        let spec = match &r.color {
            ColorSpec::PerPoint(c) if c.len() == n => {
                ColorSpec::PerPoint(Arc::new(c.iter().flat_map(|c| [*c, *c]).collect()))
            }
            ColorSpec::Values(v) if v.len() == n => {
                ColorSpec::Values(Arc::new(v.iter().flat_map(|v| [*v, *v]).collect()))
            }
            other => other.clone(),
        };
        let color = super::lines::prim_color(ctx, &spec, r.alpha as f32, 2 * n, 1, self.style_rev);
        ctx.push_data(Prim::Lines(LinesPrim {
            pts,
            color,
            width: r.linewidth as f32,
            pattern: r.linestyle.pattern(),
            cap: r.linecap,
            join: JoinStyle::Miter,
            miter_limit: std::f32::consts::FRAC_PI_3,
            segments: true,
            closed: false,
            append: false,
        }));
    }
}

/// Values for `hlines`/`vlines`/`ablines`: one number or any [`Data1D`].
pub trait RefValues {
    /// The values as f64.
    fn ref_values(self) -> Vec<f64>;
}
impl<D: Data1D> RefValues for D {
    fn ref_values(self) -> Vec<f64> {
        self.to_vec_f64()
    }
}
macro_rules! ref_scalar {
    ($($t:ty),*) => {$(
        impl RefValues for $t {
            fn ref_values(self) -> Vec<f64> { vec![self as f64] }
        }
    )*};
}
ref_scalar!(f64, f32, i32, i64, usize);

impl RefLines {
    fn with_attrs(&self, f: impl FnOnce(&mut RefLinesAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::RefLines(s) = &mut p.kind {
                f(&mut s.attrs);
                s.style_rev += 1;
            }
        });
    }

    fn create(ax: &crate::Axis, kind: RefKind) -> RefLines {
        let st = RefLinesState { kind, attrs: RefLinesAttrs::default(), style_rev: 0 };
        let id = add_to_axis(ax, PlotKind::RefLines(st));
        RefLines { sh: ax.sh.clone(), id }
    }
}

impl crate::Axis {
    /// Makie's `hlines!(ax, ys)`: horizontal lines across the axis (see the `xmin`/`xmax`
    /// attributes). They count toward the y autolimits only.
    pub fn hlines(&self, ys: impl RefValues) -> RefLines {
        RefLines::create(self, RefKind::H(Arc::new(ys.ref_values())))
    }

    /// Makie's `vlines!(ax, xs)`: vertical lines across the axis (see `ymin`/`ymax`). They count
    /// toward the x autolimits only.
    pub fn vlines(&self, xs: impl RefValues) -> RefLines {
        RefLines::create(self, RefKind::V(Arc::new(xs.ref_values())))
    }

    /// Makie's `ablines!(ax, intercepts, slopes)`: lines `y = a + b·x` across the visible x range.
    /// One intercept or slope is broadcast against several of the other.
    ///
    /// # Panics
    /// If both have more than one value and their lengths differ.
    #[track_caller]
    pub fn ablines(&self, intercepts: impl RefValues, slopes: impl RefValues) -> RefLines {
        let (a, b) = (intercepts.ref_values(), slopes.ref_values());
        let n = if a.len() == 1 || b.len() == 1 { a.len().max(b.len()) } else { a.len() };
        assert!(
            a.len() == b.len() || a.len() == 1 || b.len() == 1,
            "ablines: {} intercepts but {} slopes (use one of either, or equal lengths)",
            a.len(),
            b.len()
        );
        let pick = |v: &[f64], i: usize| if v.len() == 1 { v[0] } else { v[i] };
        let pairs =
            if a.is_empty() || b.is_empty() { vec![] } else { (0..n).map(|i| [pick(&a, i), pick(&b, i)]).collect() };
        RefLines::create(self, RefKind::AB(Arc::new(pairs)))
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::scene::SceneCache;

    fn limits(fig: &Figure) -> [f64; 4] {
        let (_, axes) = crate::scene::build(&fig.sh.snapshot(), None, &mut SceneCache::new());
        axes[0].limits
    }

    #[test]
    fn reflines_count_toward_one_dimension() {
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        ax.lines([0.0, 1.0], [0.0, 1.0]);
        let lim0 = limits(&fig);
        ax.hlines(5.0);
        let l = limits(&fig);
        assert_eq!([l[0], l[1]], [lim0[0], lim0[1]], "x unchanged");
        assert!((l[2] + 0.25).abs() < 1e-12 && (l[3] - 5.25).abs() < 1e-12, "{l:?}");
        ax.vlines([-2.0, 3.0]);
        let l = limits(&fig);
        assert!((l[0] + 2.25).abs() < 1e-12 && (l[1] - 3.25).abs() < 1e-12, "{l:?}");
        ax.ablines(100.0, 50.0);
        assert_eq!(limits(&fig), l, "ablines don't change the limits");
    }

    #[test]
    fn segments_follow_the_visible_range() {
        let fig = Figure::new();
        let ax = Axis::new(fig.at(1, 1));
        ax.limits(0.0, 10.0, -1.0, 1.0);
        let h = ax.hlines([0.5]).xmin(0.1).xmax(0.9);
        ax.ablines([0.0, 1.0], 0.1);
        let st = fig.sh.snapshot();
        let get = |i: usize| match &st.plots[i].as_ref().unwrap().kind {
            crate::plots::PlotKind::RefLines(s) => s.clone(),
            _ => unreachable!(),
        };
        let r = get(0).attrs.resolve(&Default::default(), &st.theme.globals());
        let seg = get(0).segments(&r, [0.0, 10.0, -1.0, 1.0], Scale::Identity, Scale::Identity);
        assert_eq!(seg, vec![[1.0, 0.5], [9.0, 0.5]]);
        let r = get(1).attrs.resolve(&Default::default(), &st.theme.globals());
        let seg = get(1).segments(&r, [0.0, 10.0, -1.0, 1.0], Scale::Identity, Scale::Identity);
        assert_eq!(seg, vec![[0.0, 0.0], [10.0, 1.0], [0.0, 1.0], [10.0, 2.0]]);
        drop(h);
        // Rendering works end to end (SVG).
        assert!(fig.to_svg_string(&Save::new()).unwrap().contains("<path"));
    }
}
