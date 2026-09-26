//! `use ezviz::prelude::*;` brings in the figure/axis types, plot functions, macros, colors,
//! style enums and units.

pub use crate::color::colors::*;
#[cfg(feature = "window")]
pub use crate::show_all;
pub use crate::units::*;
pub use crate::{
    Axis, Color, ColorSpec, Cycled, Data1D, Data2D, Direction, Field, Figure, Font, GridPosition, GridSize, HAlign,
    JoinStyle, LineCap, Linestyle, Marker, Normalization, Prepend, RichText, Save, Scale, Scatter, Side, Theme, VAlign,
    WONG, iter, kw, linkaxes, linkxaxes, linkyaxes, linspace, logspace, scatter, set_theme, subscript, superscript,
    theme_minimal, with_theme,
};
