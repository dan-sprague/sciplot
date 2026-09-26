//! `use ezviz::prelude::*;` brings in the figure/axis types, plot functions, macros, colors,
//! style enums and units.

pub use crate::color::colors::*;
#[cfg(feature = "window")]
pub use crate::show_all;
pub use crate::units::*;
pub use crate::{
    Axis, Band, BarPlot, Bins, Color, ColorSpec, Cycled, Data1D, Data2D, Direction, Field, Figure, Font, GridPosition,
    GridSize, HAlign, Hist, JoinStyle, LineCap, Linestyle, Marker, Normalization, Prepend, RichText, Save, Scale,
    Scatter, Side, Theme, VAlign, WONG, band, barplot, hist, iter, kw, linkaxes, linkxaxes, linkyaxes, linspace,
    logspace, scatter, set_theme, subscript, superscript, theme_minimal, with_theme,
};
pub use crate::{MinorSpec, TickFormat, TickSpec, Wilkinson};
pub use crate::{colored, rich, tex};
pub use crate::{CellCoords, Colormap, Edges, Heatmap, IntoColormap, heatmap, heatmap_xy};
