//! `use ezviz::prelude::*;` brings in the figure/axis types, plot functions, macros, colors,
//! style enums and units.

#[cfg(feature = "window")]
pub use crate::Frame;
pub use crate::color::colors::*;
#[cfg(feature = "window")]
pub use crate::show_all;
pub use crate::units::*;
pub use crate::{Aspect, AxisAspect, DataAspect, theme_dark, theme_light};
pub use crate::{
    Axis, Band, BarPlot, Bins, Color, ColorSpec, Cycled, Data1D, Data2D, Direction, Field, Figure, Font, GridPosition,
    GridSize, HAlign, Hist, JoinStyle, Label, LineCap, Linestyle, Marker, Normalization, Prepend, RichText, Save,
    Scale, Scatter, Side, Theme, VAlign, WONG, band, barplot, hist, iter, kw, linkaxes, linkxaxes, linkyaxes, linspace,
    logspace, scatter, set_theme, subscript, superscript, theme_minimal, with_theme,
};
pub use crate::{CellCoords, Colormap, Edges, Heatmap, IntoColormap, heatmap, heatmap_xy};
pub use crate::{ColorMapped, Colorbar};
pub use crate::{Contour, Contourf, ContourfMode, Extend, Levels, contour, contour_xy, contourf, contourf_xy};
pub use crate::{HLines, RefLines, VLines, ablines, hlines, vlines};
pub use crate::{Legend, LegendElement, LegendEntry, Orientation, PlotRef, Pos, axislegend};
pub use crate::{Lines, PointData, ScatterLines, lines, lines_points, scatterlines, scatterlines_points};
#[cfg(feature = "window")]
pub use crate::{Live, Screen};
pub use crate::{MinorSpec, TickFormat, TickSpec, Wilkinson};
pub use crate::{colored, rich, tex};
