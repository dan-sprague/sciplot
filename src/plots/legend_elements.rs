//! Legend elements: what a plot looks like in a legend patch (Makie's `LineElement`,
//! `MarkerElement`, `PolyElement` and the heatmap's `ImageElement`).
//!
//! Every plot type describes itself through `PlotImpl::legend_elements`; colors resolve exactly
//! like the plot's own `emit` (same palette, same cycle index), and colors that go through a
//! colormap fall back to the Legend's defaults like Makie's `extract_color`.

use super::ColorSpec;
use crate::color::Color;
use crate::style::{Linestyle, Marker};
use crate::theme::{Globals, Theme};

/// One graphical element drawn in a legend entry's patch (a `patchsize` box, 20 × 20 by default).
/// Several elements are drawn on top of each other (e.g. scatterlines: a line and a marker).
#[derive(Clone, Debug, PartialEq)]
pub enum LegendElement {
    /// A horizontal line across the patch at mid height (Makie's `linepoints = [(0, 0.5), (1, 0.5)]`).
    Line { color: Color, linewidth: f64, linestyle: Linestyle },
    /// One marker at the patch center (Makie's `markerpoints = [(0.5, 0.5)]`).
    Marker { color: Color, marker: Marker, markersize: f64, strokecolor: Color, strokewidth: f64 },
    /// A rectangle filling the whole patch (Makie's `polypoints` = the unit square).
    Poly { color: Color, strokecolor: Color, strokewidth: f64 },
    /// A 2 × 2 grid of cells filling the patch, `[bottom-left, bottom-right, top-left, top-right]`
    /// (Makie's heatmap element: values `[0 0.3; 0.6 1]` through the plot's colormap).
    Cells { colors: [Color; 4] },
}

/// Legend element defaults for values that are not a single color (Makie theme `linecolor`,
/// `markercolor`, `patchcolor`).
pub(crate) const DEFAULT_LINECOLOR: Color = Color::rgb(0.0, 0.0, 0.0);
pub(crate) const DEFAULT_MARKERCOLOR: Color = Color::rgb(0.0, 0.0, 0.0);
pub(crate) const DEFAULT_POLYCOLOR: Color = Color::rgb(0.4, 0.4, 0.4);

/// What a plot needs to describe its legend elements.
pub(crate) struct LegendCtx<'a> {
    pub theme: &'a Theme,
    pub g: &'a Globals,
    /// The plot's index in its axis' cycle group (as in `PlotCtx::cycle`).
    pub cycle: usize,
}

impl LegendCtx<'_> {
    /// A single color for `spec` (line/marker palette, or the fill palette with `patch`), with
    /// `alpha` applied; `fallback` for per-point colors and colormapped values (Makie's
    /// `extract_color` / `choose_scalar`).
    pub fn color(&self, spec: &ColorSpec, patch: bool, alpha: f64, fallback: Color) -> Color {
        let pal = if patch { &self.g.patchpalette } else { &self.g.palette };
        match crate::scene::resolve_color(spec, self.cycle, pal) {
            Some(c) => c.with_alpha(c.a * alpha as f32),
            None => fallback.with_alpha(fallback.a * alpha as f32),
        }
    }
}
