//! Makie's unitless size model: 1 unit = 1 CSS pixel = 1/96 inch = 0.75 pt.
//!
//! ```
//! use sciplot::units::*;
//! assert_eq!(12.0 * PT, 16.0);
//! assert_eq!(4.0 * INCH, 384.0);
//! ```
//!
//! Provenance: the unit convention follows Makie 0.24.14 `src/theming.jl` (CairoMakie
//! `pt_per_unit = 0.75`) and CairoMakie 0.15.14 `src/screen.jl` (`pt_per_unit`, `css_px_per_unit`).
//! MIT licensed; see THIRD_PARTY_NOTICES.md.

/// One CSS pixel (the base unit).
pub const PX: f64 = 1.0;
/// One typographic point.
pub const PT: f64 = 96.0 / 72.0;
/// One inch.
pub const INCH: f64 = 96.0;
/// One centimetre.
pub const CM: f64 = 96.0 / 2.54;
/// One millimetre.
pub const MM: f64 = CM / 10.0;
