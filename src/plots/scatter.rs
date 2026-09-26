//! `scatter`: markers at points.

use super::{ColorSpec, PlotKind, add_to_axis, plot_common, zip_xy};
use crate::attrs::attributes;
use crate::color::Color;
use crate::data::Data1D;
use crate::figure::{Dirty, FigShared, PlotId};
use crate::style::Marker;
use std::sync::Arc;

/// A scatter plot handle (Makie's `Scatter`).
#[derive(Clone)]
pub struct Scatter {
    pub(crate) sh: Arc<FigShared>,
    pub(crate) id: PlotId,
}

#[derive(Clone, Debug)]
pub(crate) struct ScatterState {
    pub pos: Arc<Vec<[f64; 2]>>,
    pub attrs: ScatterAttrs,
}

attributes! {
    Scatter(ScatterAttrs, ScatterResolved, ScatterTheme) via with_attrs {
        /// A color, `Cycled(i)`, per-point colors, or values mapped through the colormap.
        color: ColorSpec = |_| ColorSpec::Auto, STYLE;
        marker: Marker = |_| Marker::Circle, STYLE;
        /// Marker size in units (Makie default 9; a `Circle` marker is 0.705 × this wide).
        markersize: f64 = |g| g.markersize, STYLE;
        strokecolor: Color = |_| Color::rgb(0.0, 0.0, 0.0), STYLE;
        strokewidth: f64 = |_| 0.0, STYLE;
        /// Opacity multiplier.
        alpha: f64 = |_| 1.0, STYLE;
        /// Marker rotation in radians.
        rotation: f64 = |_| 0.0, STYLE;
    }
}

plot_common!(Scatter);

impl Scatter {
    fn with_attrs(&self, f: impl FnOnce(&mut ScatterAttrs), dirty: u8) {
        self.with_slot(dirty, |p| {
            if let PlotKind::Scatter(s) = &mut p.kind {
                f(&mut s.attrs)
            }
        });
    }

    pub(crate) fn create(ax: &crate::Axis, pos: Vec<[f64; 2]>) -> Scatter {
        let st = ScatterState {
            pos: Arc::new(pos),
            attrs: ScatterAttrs::default(),
        };
        let id = add_to_axis(ax, PlotKind::Scatter(st));
        Scatter {
            sh: ax.sh.clone(),
            id,
        }
    }

    /// Replaces the points (the lengths of x and y must match).
    #[track_caller]
    pub fn set_data(&self, x: impl Data1D, y: impl Data1D) -> Scatter {
        let pos = Arc::new(zip_xy("Scatter::set_data", x.to_vec_f64(), y.to_vec_f64()));
        self.with_slot(Dirty::DATA | Dirty::LIMITS, |p| {
            if let PlotKind::Scatter(s) = &mut p.kind {
                s.pos = pos;
                p.data_rev += 1;
            }
        });
        self.clone()
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        match self.sh.state.lock().plot(self.id).map(|p| &p.kind) {
            Some(PlotKind::Scatter(s)) => s.pos.len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl crate::Axis {
    /// Makie's `scatter!(ax, x, y)`.
    #[track_caller]
    pub fn scatter(&self, x: impl Data1D, y: impl Data1D) -> Scatter {
        Scatter::create(self, zip_xy("scatter", x.to_vec_f64(), y.to_vec_f64()))
    }
}

impl crate::GridPosition {
    /// Makie's `scatter(fig[r, c], x, y)`: a new Axis at this position with a scatter plot.
    #[track_caller]
    pub fn scatter(&self, x: impl Data1D, y: impl Data1D) -> Scatter {
        crate::Axis::new(self.clone()).scatter(x, y)
    }
}

/// Makie's `scatter(x, y)`: a new Figure and Axis with a scatter plot. Returns the plot handle;
/// call `.save(..)` or `.show()` on it.
///
/// ```no_run
/// let x: Vec<f64> = (0..100).map(|i| i as f64).collect();
/// ezviz::scatter(&x, x.iter().map(|v| v.sin())).save("scatter.png").unwrap();
/// ```
#[track_caller]
#[must_use = "this creates a new Figure; call .save(..) or .show() on it"]
pub fn scatter(x: impl Data1D, y: impl Data1D) -> Scatter {
    crate::Figure::new().at(1, 1).scatter(x, y)
}
