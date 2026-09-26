//! Makie's unitless size model: 1 unit = 1 CSS pixel = 1/96 inch = 0.75 pt.
//!
//! ```
//! use ezviz::units::*;
//! assert_eq!(12.0 * PT, 16.0);
//! assert_eq!(4.0 * INCH, 384.0);
//! ```

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
