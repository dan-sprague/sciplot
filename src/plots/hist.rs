//! `hist`: histogram (Makie's recipe on StatsBase semantics: equal-width bins, closed on the left).

use super::bars::{Bar, BarLayout, bars_bounds, emit_bar_strokes, emit_bars, layout_bars, pick_bar};
use super::{ColorSpec, PlotImpl, PlotKind, add_to_axis, is_auto, plot_common};
use crate::attrs::{Conv, attributes};
use crate::color::Color;
use crate::data::{Data1D, Num};
use crate::figure::{Dirty, FigShared, PlotId};
use crate::scene::PlotCtx;
use crate::style::{Direction, Normalization};
use crate::transform::Scale;
use std::sync::Arc;

/// A histogram handle (Makie's `Hist`).
#[derive(Clone)]
pub struct Hist {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

/// Histogram bins: a count (equal-width over the data range) or explicit edges.
#[derive(Clone, Debug, PartialEq)]
pub enum Bins {
    Count(usize),
    Edges(Vec<f64>),
}

impl<N: Num> Conv<Bins> for N {
    fn conv(self) -> Bins {
        Bins::Count(self.to_f64().max(1.0) as usize)
    }
}
impl Conv<Bins> for Vec<f64> {
    fn conv(self) -> Bins {
        Bins::Edges(self)
    }
}
impl Conv<Bins> for &[f64] {
    fn conv(self) -> Bins {
        Bins::Edges(self.to_vec())
    }
}
impl Conv<Bins> for &Vec<f64> {
    fn conv(self) -> Bins {
        Bins::Edges(self.clone())
    }
}
impl Conv<Bins> for Bins {
    fn conv(self) -> Bins {
        self
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HistState {
    pub values: Arc<Vec<f64>>,
    pub weights: Option<Arc<Vec<f64>>>,
    pub attrs: HistAttrs,
}

attributes! {
    Hist(HistAttrs, HistResolved, HistTheme) via with_attrs {
        /// Number of equal-width bins (Makie default 15) or explicit edges.
        bins: Bins = |_| Bins::Count(15), LIMITS;
        /// `None` (counts), `Pdf`, `Density` or `Probability`.
        normalization: Normalization = |_| Normalization::None, LIMITS;
        /// Scale the tallest bar to this height.
        scale_to: Option<f64> = |_| None, LIMITS;
        color: ColorSpec = |_| ColorSpec::Auto, STYLE;
        /// Fraction of the bin width left empty (Makie default 0 for hist).
        gap: f64 = |_| 0.0, LIMITS;
        offset: f64 = |_| 0.0, LIMITS;
        fillto: Option<f64> = |_| None, LIMITS;
        direction: Direction = |_| Direction::Y, LIMITS;
        /// Width of each bar's outline in units, centered on its edges (Makie default 0).
        strokewidth: f64 = |_| 0.0, STYLE;
        /// Outline color (Makie default black).
        strokecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        /// Opacity multiplier for fill and outline.
        alpha: f64 = |_| 1.0, STYLE;
    }
}

plot_common!(Hist);

/// Bin edges like Makie's `pick_hist_edges`.
pub(crate) fn hist_edges(values: &[f64], bins: &Bins) -> Vec<f64> {
    match bins {
        Bins::Edges(e) => e.clone(),
        Bins::Count(n) => {
            let Some((mi, ma)) = crate::data::finite_extrema(values.iter().copied()) else { return vec![] };
            if mi == ma {
                return vec![mi - 0.5, mi + 0.5];
            }
            let ma = ma.next_up();
            let n = (*n).max(1);
            (0..=n).map(|i| if i == n { ma } else { mi + (ma - mi) * i as f64 / n as f64 }).collect()
        }
    }
}

/// Weighted counts per bin, closed on the left (`[e_i, e_{i+1})`); NaNs and out-of-range dropped.
pub(crate) fn hist_counts(values: &[f64], weights: Option<&[f64]>, edges: &[f64]) -> Vec<f64> {
    let nb = edges.len().saturating_sub(1);
    let mut w = vec![0.0; nb];
    if nb == 0 {
        return w;
    }
    for (i, &v) in values.iter().enumerate() {
        if !v.is_finite() || v < edges[0] || v >= edges[nb] {
            continue;
        }
        // Last edge <= v.
        let k = edges.partition_point(|e| *e <= v) - 1;
        if k < nb {
            w[k] += weights.map_or(1.0, |ws| ws.get(i).copied().unwrap_or(0.0));
        }
    }
    w
}

/// Applies Makie/StatsBase normalization.
pub(crate) fn normalize(w: &mut [f64], edges: &[f64], norm: Normalization, scale_to: Option<f64>) {
    let total: f64 = w.iter().sum();
    for (i, v) in w.iter_mut().enumerate() {
        let dx = edges[i + 1] - edges[i];
        *v = match norm {
            Normalization::None => *v,
            Normalization::Density => *v / dx,
            Normalization::Probability => *v / total,
            Normalization::Pdf => *v / (total * dx),
        };
    }
    if let Some(s) = scale_to {
        let m = w.iter().fold(0.0f64, |a, b| a.max(b.abs()));
        if m > 0.0 {
            w.iter_mut().for_each(|v| *v *= s / m);
        }
    }
}

impl HistState {
    /// The bin edges and the (normalized) bar heights; both empty without bins.
    fn bins(&self, r: &HistResolved) -> (Vec<f64>, Vec<f64>) {
        let edges = hist_edges(&self.values, &r.bins);
        if edges.len() < 2 {
            return (vec![], vec![]);
        }
        let mut w = hist_counts(&self.values, self.weights.as_deref().map(|v| v.as_slice()), &edges);
        normalize(&mut w, &edges, r.normalization, r.scale_to);
        (edges, w)
    }

    fn bars(&self, r: &HistResolved, value_scale: Scale) -> Vec<Bar> {
        let (edges, w) = self.bins(r);
        if edges.len() < 2 {
            return vec![];
        }
        let centers: Vec<f64> = edges.windows(2).map(|e| 0.5 * (e[0] + e[1])).collect();
        let fillto = r.fillto.unwrap_or_else(|| {
            if value_scale.is_log() {
                w.iter().copied().filter(|v| *v > 0.0).fold(f64::INFINITY, f64::min) / 2.0
            } else {
                0.0
            }
        });
        let mut bars = layout_bars(
            &centers,
            &w,
            &BarLayout { width: None, gap: r.gap, dodge: None, dodge_gap: 0.0, stack: None, fillto, offset: r.offset },
        );
        // Bins may have unequal widths: each bar spans its own bin (times 1 - gap).
        for (b, e) in bars.iter_mut().zip(edges.windows(2)) {
            let c = 0.5 * (e[0] + e[1]);
            let hw = 0.5 * (e[1] - e[0]) * (1.0 - r.gap);
            b.x0 = c - hw;
            b.x1 = c + hw;
        }
        bars
    }
}

impl PlotImpl for HistState {
    fn cycle_group(&self) -> &'static str {
        "hist"
    }

    fn color_is_auto(&self, theme: &crate::theme::Theme) -> bool {
        is_auto(self.attrs.color.as_ref(), theme.hist.color.as_ref())
    }

    fn legend_elements(&self, ctx: &super::legend_elements::LegendCtx<'_>) -> Vec<super::LegendElement> {
        let r = self.attrs.resolve(&ctx.theme.hist, ctx.g);
        let (a, sc) = (r.alpha as f32, r.strokecolor);
        vec![super::LegendElement::Poly {
            color: ctx.color(&r.color, true, r.alpha, super::legend_elements::DEFAULT_POLYCOLOR),
            strokecolor: sc.with_alpha(sc.a * a),
            strokewidth: r.strokewidth,
        }]
    }

    fn data_bounds(&self, xs: Scale, ys: Scale) -> Option<[f64; 4]> {
        let r = self.attrs.resolve(&Default::default(), &crate::theme::Globals::default());
        let vscale = if r.direction == Direction::Y { ys } else { xs };
        bars_bounds(&self.bars(&r, vscale), r.direction, xs, ys)
    }

    fn emit(&self, ctx: &mut PlotCtx<'_>) {
        let r = self.attrs.resolve(&ctx.theme.hist, ctx.g);
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

    /// Makie's hist is a barplot of (bin center, height): the same inspection.
    fn pick(&self, ctx: &mut super::pick::PickCtx<'_>) -> Option<super::pick::Hover> {
        let r = self.attrs.resolve(&ctx.theme.hist, ctx.g);
        let a = &ctx.axis.attrs;
        let bars = self.bars(&r, if r.direction == Direction::Y { a.yscale } else { a.xscale });
        let (edges, w) = self.bins(&r);
        pick_bar(ctx, &bars, r.direction, |i| Some([0.5 * (edges.get(i)? + edges.get(i + 1)?), *w.get(i)?]))
    }
}

impl Hist {
    fn with_attrs(&self, f: impl FnOnce(&mut HistAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Hist(s) = &mut p.kind {
                f(&mut s.attrs)
            }
        });
    }

    /// Replaces the samples.
    pub fn set_data(&self, values: impl Data1D) -> Hist {
        let v = Arc::new(values.to_vec_f64());
        self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            if let PlotKind::Hist(s) = &mut p.kind {
                s.values = v;
                p.data_rev += 1;
            }
        });
        self.clone()
    }

    /// Per-sample weights (same length as the samples).
    pub fn weights(&self, w: impl Data1D) -> Hist {
        let w = Arc::new(w.to_vec_f64());
        self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            if let PlotKind::Hist(s) = &mut p.kind {
                s.weights = Some(w);
                p.data_rev += 1;
            }
        });
        self.clone()
    }

    /// The current bin edges.
    pub fn edges(&self) -> Vec<f64> {
        match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Hist(s)) => {
                let r = s.attrs.resolve(&Default::default(), &crate::theme::Globals::default());
                hist_edges(&s.values, &r.bins)
            }
            _ => vec![],
        }
    }

    /// The current (normalized) bar heights.
    pub fn heights(&self) -> Vec<f64> {
        match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Hist(s)) => {
                s.bins(&s.attrs.resolve(&Default::default(), &crate::theme::Globals::default())).1
            }
            _ => vec![],
        }
    }
}

impl crate::Axis {
    /// Makie's `hist!(ax, values)`.
    pub fn hist(&self, values: impl Data1D) -> Hist {
        let st = HistState { values: Arc::new(values.to_vec_f64()), weights: None, attrs: HistAttrs::default() };
        Hist { sh: self.sh.clone(), id: add_to_axis(self, PlotKind::Hist(st)) }
    }
}

impl crate::GridPosition {
    /// Makie's `hist(fig[r, c], values)`.
    pub fn hist(&self, values: impl Data1D) -> Hist {
        crate::Axis::new(self.clone()).hist(values)
    }
}

/// Makie's `hist(values)`: a new Figure and Axis with a histogram.
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn hist(values: impl Data1D) -> Hist {
    crate::Figure::new().at(1, 1).hist(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_and_counts() {
        let v = [0.0, 1.0, 2.0, 3.0, 4.0];
        let e = hist_edges(&v, &Bins::Count(4));
        assert_eq!(e.len(), 5);
        assert_eq!(e[0], 0.0);
        assert!(e[4] > 4.0, "max is included via nextfloat");
        let c = hist_counts(&v, None, &e);
        assert_eq!(c.iter().sum::<f64>(), 5.0);
        let mut p = c.clone();
        normalize(&mut p, &e, Normalization::Pdf, None);
        let integral: f64 = p.iter().zip(e.windows(2)).map(|(h, w)| h * (w[1] - w[0])).sum();
        assert!((integral - 1.0).abs() < 1e-12);
        assert_eq!(hist_edges(&[2.0, 2.0], &Bins::Count(10)), vec![1.5, 2.5]);
    }
}
