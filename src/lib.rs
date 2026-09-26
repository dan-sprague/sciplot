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

pub use attrs::Conv;
pub use blocks::axis::AxisTheme;
pub use blocks::{Axis, linkaxes, linkxaxes, linkyaxes};
pub use color::{Color, IntoColor, WONG, colors};
pub use data::{Data1D, Data2D, Field, Iter, Num, Scalar, iter, linspace, logspace};
pub use error::{Error, Result};
pub use figure::{Figure, GridPosition, GridSize, IntoSpan, Prepend, RgbaImage, Save, Side, Span};
pub use plots::scatter::{ScatterTheme, scatter};
pub use plots::{ColorSpec, Cycled, Scatter};
pub use style::{Direction, HAlign, JoinStyle, LineCap, Linestyle, Marker, Normalization, VAlign};
pub use text::{Font, RichText, TextSpan, subscript, superscript};
pub use theme::{Theme, current_theme, reset_theme, set_theme, theme_minimal, with_theme};
pub use transform::Scale;
#[cfg(feature = "window")]
pub use window::show_all;

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
    ok::<GridPosition>();
};
