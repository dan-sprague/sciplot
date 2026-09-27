//! `barplot`: bars from a baseline (`fillto`) to heights, with dodge/stack and categorical x.

use super::bars::{Bar, BarLayout, bars_bounds, emit_bar_strokes, emit_bars, layout_bars, pick_bar};
use super::{ColorSpec, PlotImpl, PlotKind, add_to_axis, is_auto, plot_common};
use crate::attrs::attributes;
use crate::color::Color;
use crate::data::Data1D;
use crate::figure::{Dirty, FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::style::Direction;
use crate::transform::Scale;
use std::sync::Arc;

/// A barplot handle (Makie's `BarPlot`).
#[derive(Clone)]
pub struct BarPlot {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct BarPlotState {
    pub x: Arc<Vec<f64>>,
    pub h: Arc<Vec<f64>>,
    /// Category names when x was given as strings (x = 1..=n in order of appearance).
    pub categories: Option<Arc<Vec<String>>>,
    pub dodge: Option<Arc<Vec<usize>>>,
    pub stack: Option<Arc<Vec<usize>>>,
    pub attrs: BarPlotAttrs,
}

attributes! {
    BarPlot(BarPlotAttrs, BarPlotResolved, BarPlotTheme) via with_attrs {
        /// A color, `Cycled(i)`, or per-bar colors. Defaults to the cycled patch color.
        color: ColorSpec = |_| ColorSpec::Auto, STYLE;
        /// Fraction of the bar width left empty (Makie default 0.2).
        gap: f64 = |_| 0.2, LIMITS;
        /// Bar width in data units (default: minimum spacing of x).
        width: Option<f64> = |_| None, LIMITS;
        dodge_gap: f64 = |_| 0.03, LIMITS;
        /// Baseline (default 0; on a log axis half the smallest positive height).
        fillto: Option<f64> = |_| None, LIMITS;
        offset: f64 = |_| 0.0, LIMITS;
        /// `Direction::Y` (vertical bars, default) or `Direction::X` (horizontal).
        direction: Direction = |_| Direction::Y, LIMITS;
        /// Width of each bar's outline in units, centered on its edges (Makie default 0).
        strokewidth: f64 = |_| 0.0, STYLE;
        /// Outline color (Makie default black).
        strokecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        /// Opacity multiplier for fill and outline.
        alpha: f64 = |_| 1.0, STYLE;
    }
}

plot_common!(BarPlot);

/// The x positions of bars: numbers, or category names (`["CG", "GMRES"]`) placed at 1..=n in order
/// of appearance with the names as tick labels.
pub trait BarX {
    #[doc(hidden)]
    fn bar_x(self) -> (Vec<f64>, Option<Vec<String>>);
}

impl<D: Data1D> BarX for D {
    fn bar_x(self) -> (Vec<f64>, Option<Vec<String>>) {
        (self.to_vec_f64(), None)
    }
}

fn categorical<S: AsRef<str>>(names: impl IntoIterator<Item = S>) -> (Vec<f64>, Option<Vec<String>>) {
    let mut cats: Vec<String> = Vec::new();
    let mut x = Vec::new();
    for n in names {
        let n = n.as_ref();
        let i = match cats.iter().position(|c| c == n) {
            Some(i) => i,
            None => {
                cats.push(n.to_string());
                cats.len() - 1
            }
        };
        x.push((i + 1) as f64);
    }
    (x, Some(cats))
}

macro_rules! barx_strings {
    ($($t:ty),*) => {$(
        impl BarX for $t {
            fn bar_x(self) -> (Vec<f64>, Option<Vec<String>>) { categorical(self.iter()) }
        }
    )*};
}
barx_strings!(&[&str], &Vec<&str>, Vec<&str>, &[String], &Vec<String>, Vec<String>);
impl<const N: usize> BarX for [&str; N] {
    fn bar_x(self) -> (Vec<f64>, Option<Vec<String>>) {
        categorical(self.iter())
    }
}
impl<const N: usize> BarX for &[&str; N] {
    fn bar_x(self) -> (Vec<f64>, Option<Vec<String>>) {
        categorical(self.iter())
    }
}

impl BarPlotState {
    fn fillto(&self, r: &BarPlotResolved, value_scale: Scale) -> f64 {
        r.fillto.unwrap_or_else(|| {
            if value_scale.is_log() {
                self.h.iter().copied().filter(|v| *v > 0.0).fold(f64::INFINITY, f64::min) / 2.0
            } else {
                0.0
            }
        })
    }

    fn bars(&self, r: &BarPlotResolved, value_scale: Scale) -> Vec<Bar> {
        layout_bars(
            &self.x,
            &self.h,
            &BarLayout {
                width: r.width,
                gap: r.gap,
                dodge: self.dodge.as_deref().map(|v| v.as_slice()),
                dodge_gap: r.dodge_gap,
                stack: self.stack.as_deref().map(|v| v.as_slice()),
                fillto: self.fillto(r, value_scale),
                offset: r.offset,
            },
        )
    }
}

impl PlotImpl for BarPlotState {
    fn cycle_group(&self) -> &'static str {
        "barplot"
    }

    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool {
        is_auto(self.attrs.color.as_ref(), theme.barplot.color.as_ref())
    }

    fn data_bounds(&self, xs: Scale, ys: Scale) -> Option<[f64; 4]> {
        let r = self.attrs.resolve(&Default::default(), &crate::theme::Globals::default());
        let vscale = if r.direction == Direction::Y { ys } else { xs };
        bars_bounds(&self.bars(&r, vscale), r.direction, xs, ys)
    }

    fn categories(&self) -> Option<(bool, Arc<Vec<String>>)> {
        let horizontal = self.attrs.direction == Some(Direction::X);
        self.categories.clone().map(|c| (!horizontal, c))
    }

    fn legend_elements(&self, ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        let r = self.attrs.resolve(&ctx.theme.barplot, ctx.g);
        let (a, sc) = (r.alpha as f32, r.strokecolor);
        vec![super::LegendElement::Poly {
            color: ctx.color(&r.color, true, r.alpha, super::legend_elements::DEFAULT_POLYCOLOR),
            strokecolor: sc.with_alpha(sc.a * a),
            strokewidth: r.strokewidth,
        }]
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.barplot, ctx.g);
        let vscale = if r.direction == Direction::Y { ctx.axis.attrs.yscale } else { ctx.axis.attrs.xscale };
        let bars = self.bars(&r, vscale);
        let alpha = r.alpha as f32;
        let colors: Vec<Color> = match ctx.solid_color(&r.color, true) {
            Some(c) => vec![c.with_alpha(c.a * alpha)],
            None => match &r.color {
                ColorSpec::PerPoint(cs) => cs.iter().map(|c| c.with_alpha(c.a * alpha)).collect(),
                _ => vec![ctx.g.patchpalette[0]],
            },
        };
        emit_bars(ctx, 0, &bars, &colors, r.direction);
        let sc = r.strokecolor;
        emit_bar_strokes(ctx, &bars, sc.with_alpha(sc.a * alpha), r.strokewidth, r.direction);
    }

    fn pick(&self, ctx: &mut super::pick::PickCtx<'_>) -> Option<super::pick::Hover> {
        let r = self.attrs.resolve(&ctx.theme.barplot, ctx.g);
        let a = &ctx.axis.attrs;
        let bars = self.bars(&r, if r.direction == Direction::Y { a.yscale } else { a.xscale });
        pick_bar(ctx, &bars, r.direction, |i| Some([*self.x.get(i)?, *self.h.get(i)?]))
    }
}

impl BarPlot {
    fn with_attrs(&self, f: impl FnOnce(&mut BarPlotAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::BarPlot(s) = &mut p.kind {
                f(&mut s.attrs)
            }
        });
    }

    fn with_state(&self, dirty: u8, f: impl FnOnce(&mut BarPlotState)) {
        self.with_slot(dirty, |p| {
            if let PlotKind::BarPlot(s) = &mut p.kind {
                f(s);
                p.data_rev += 1;
            }
        });
    }

    #[track_caller]
    pub(crate) fn create(ax: &crate::Axis, x: impl BarX, h: impl Data1D) -> BarPlot {
        let (x, categories) = x.bar_x();
        let h = h.to_vec_f64();
        assert!(x.len() == h.len(), "barplot: {} positions but {} heights", x.len(), h.len());
        let st = BarPlotState {
            x: Arc::new(x),
            h: Arc::new(h),
            categories: categories.map(Arc::new),
            dodge: None,
            stack: None,
            attrs: BarPlotAttrs::default(),
        };
        BarPlot { sh: ax.sh.clone(), id: add_to_axis(ax, PlotKind::BarPlot(st)) }
    }

    /// Replaces positions and heights.
    #[track_caller]
    pub fn set_data(&self, x: impl BarX, h: impl Data1D) -> BarPlot {
        let (x, cats) = x.bar_x();
        let h = h.to_vec_f64();
        assert!(x.len() == h.len(), "BarPlot::set_data: {} positions but {} heights", x.len(), h.len());
        self.with_state(Dirty::DATA | Dirty::LIMITS, |s| {
            s.x = Arc::new(x);
            s.h = Arc::new(h);
            s.categories = cats.map(Arc::new);
        });
        self.clone()
    }

    /// Replaces the heights, keeping the positions.
    #[track_caller]
    pub fn set_heights(&self, h: impl Data1D) -> BarPlot {
        let h = h.to_vec_f64();
        self.with_state(Dirty::DATA | Dirty::LIMITS, |s| {
            if h.len() == s.x.len() {
                s.h = Arc::new(h);
            } else {
                crate::warn_once("BarPlot::set_heights: length differs from the number of bars; ignored");
            }
        });
        self.clone()
    }

    /// Group index (1-based) of each bar: bars at the same x with different groups sit side by side.
    pub fn dodge(&self, groups: impl Data1D) -> BarPlot {
        let g: Vec<usize> = groups.to_vec_f64().into_iter().map(|v| v.max(1.0) as usize).collect();
        self.with_state(Dirty::LIMITS, |s| s.dodge = Some(Arc::new(g)));
        self.clone()
    }

    /// Group index (1-based) of each bar: bars at the same x with different groups stack.
    pub fn stack(&self, groups: impl Data1D) -> BarPlot {
        let g: Vec<usize> = groups.to_vec_f64().into_iter().map(|v| v.max(1.0) as usize).collect();
        self.with_state(Dirty::LIMITS, |s| s.stack = Some(Arc::new(g)));
        self.clone()
    }
}

impl crate::Axis {
    /// Makie's `barplot!(ax, x, heights)`. `x` may be numbers or category names.
    #[track_caller]
    pub fn barplot(&self, x: impl BarX, heights: impl Data1D) -> BarPlot {
        BarPlot::create(self, x, heights)
    }

    /// Makie's `barplot!(ax, heights)`: bars at x = 1..=n.
    #[track_caller]
    pub fn barplot_heights(&self, heights: impl Data1D) -> BarPlot {
        let h = heights.to_vec_f64();
        let x: Vec<f64> = (1..=h.len()).map(|i| i as f64).collect();
        BarPlot::create(self, x, h)
    }
}

impl crate::GridPosition {
    /// Makie's `barplot(fig[r, c], x, heights)`.
    #[track_caller]
    pub fn barplot(&self, x: impl BarX, heights: impl Data1D) -> BarPlot {
        crate::Axis::new(self.clone()).barplot(x, heights)
    }
}

/// Makie's `barplot(x, heights)`: a new Figure and Axis with bars.
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn barplot(x: impl BarX, heights: impl Data1D) -> BarPlot {
    crate::Figure::new().at(1, 1).barplot(x, heights)
}
