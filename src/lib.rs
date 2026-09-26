//! # ezviz: Makie-style plotting for Rust
//!
//! A small, Makie-flavored plotting API: a `Figure` holds a grid layout, blocks like [`Axis`] sit
//! in grid cells, and plots are drawn into axes. Rendering is GPU-accelerated (wgpu) for both
//! interactive windows and PNG export; SVG export is a separate vector backend.
//!
//! ```no_run
//! use ezviz::prelude::*;
//!
//! let x: Vec<f64> = (0..200).map(|i| i as f64 / 20.0).collect();
//! // One-liner: new Figure + Axis + plot.
//! scatter(&x, x.iter().map(|x| x.sin())).save("sin.png").unwrap();
//!
//! // Explicit figure with Makie-style keyword sugar.
//! let fig = Figure::new();
//! let ax = Axis!(fig.at(1, 1); title = "damped", xlabel = "t (s)", ylabel = "u (V)");
//! scatter!(ax, &x, x.iter().map(|t| (-0.3 * t).exp() * t.cos()); markersize = 6);
//! fig.save("damped.png").unwrap();
//! ```
//!
//! Grid positions are 1-based and inclusive like Makie (`fig.at(2, 1..=2)`); anything that
//! indexes your own data (hover readouts, `Field`) is 0-based like Rust.

#![deny(unsafe_code)]
// Scaffolding for later milestones; removed at M10.
#![allow(dead_code, irrefutable_let_patterns)]

pub(crate) mod attrs;
pub(crate) mod blocks;
pub mod color;
pub mod data;
mod error;
pub(crate) mod figure;
pub(crate) mod layout;
#[macro_use]
mod macros;
pub(crate) mod plots;
pub(crate) mod render;
pub(crate) mod scene;
pub mod style;
pub mod text;
pub(crate) mod theme;
pub(crate) mod ticks;
pub mod transform;
pub mod units;
#[cfg(feature = "window")]
mod window;

pub mod prelude;
#[doc(hidden)]
pub mod testing;

#[doc(hidden)]
pub use macros::{__as_axis, AsAxis};
/// GPU portability hooks (WebGL2-limited contexts, shader sources) for `tests/portability.rs`.
#[doc(hidden)]
pub use render::gpu::testing as gpu_testing;

pub use attrs::Conv;
pub use blocks::axis::AxisTheme;
pub use blocks::label::LabelTheme;
pub use blocks::{Axis, Label, linkaxes, linkxaxes, linkyaxes};
pub use color::{Color, IntoColor, WONG, colors};
pub use color::{Colormap, IntoColormap};
pub use data::PointData;
pub use data::{CellCoords, Edges};
pub use data::{Data1D, Data2D, Field, Iter, Num, Scalar, iter, linspace, logspace};
pub use error::{Error, Result};
pub use figure::{Figure, GridPosition, GridSize, IntoSpan, Prepend, RgbaImage, Save, Side, Span};
pub use plots::Heatmap;
pub use plots::band::{BandTheme, band};
pub use plots::barplot::{BarPlotTheme, barplot};
pub use plots::heatmap::{HeatmapTheme, heatmap, heatmap_xy};
pub use plots::hist::{HistTheme, hist};
pub use plots::lines::{LinesTheme, lines, lines_points};
pub use plots::scatter::{ScatterTheme, scatter};
pub use plots::scatterlines::{ScatterLinesTheme, scatterlines, scatterlines_points};
pub use plots::{Band, BarPlot, BarX, Bins, ColorSpec, Cycled, Hist, IntoTexts, Scatter, TextPlot};
pub use plots::{Lines, ScatterLines};
pub use style::{Direction, HAlign, JoinStyle, LineCap, Linestyle, Marker, Normalization, VAlign};
pub use text::{Font, RichText, TextSpan, subscript, superscript};
pub use text::{colored, tex};
pub use theme::{Theme, current_theme, reset_theme, set_theme, theme_minimal, with_theme};
#[doc(hidden)]
pub use ticks::testing as __ticks;
pub use ticks::{LabelFn, MinorSpec, TickFn, TickFormat, TickSpec, Wilkinson, format_ticks_auto, format_with};
pub use transform::Scale;
/// Window interactions as a pure state machine (exposed for tests).
#[cfg(feature = "window")]
#[doc(hidden)]
pub use window::interact;
#[cfg(feature = "window")]
pub use window::show_all;
/// Scripted-window hooks (feature `testing`).
#[cfg(feature = "testing")]
#[doc(hidden)]
pub use window::testing as window_testing;
#[cfg(feature = "window")]
pub use window::{Live, Screen};

/// Logs a warning once per distinct message.
pub(crate) fn warn_once(msg: &'static str) {
    use parking_lot::Mutex;
    use std::collections::HashSet;
    static SEEN: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    if SEEN.lock().get_or_insert_with(HashSet::new).insert(msg) {
        log::warn!("ezviz: {msg}");
        eprintln!("ezviz warning: {msg}");
    }
}

// Every handle is cheap to clone and can be sent to a simulation thread.
const _: () = {
    const fn ok<T: Send + Sync + Clone + 'static>() {}
    ok::<Figure>();
    ok::<Axis>();
    ok::<Scatter>();
    ok::<Heatmap>();
    ok::<Lines>();
    ok::<ScatterLines>();
    ok::<GridPosition>();
    #[cfg(feature = "window")]
    ok_send_sync::<Live>();
};

const fn ok_send_sync<T: Send + Sync>() {}
