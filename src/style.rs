//! Small style enums shared by plots and blocks.

use crate::attrs::conv_identity;

/// Marker shapes. Symbol markers use Makie's exact geometry (`Circle` is 0.705 × markersize wide);
/// `FullCircle` and `FullRect` fill the whole markersize like Makie's `Circle`/`Rect` types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Marker {
    #[default]
    Circle,
    Rect,
    Diamond,
    Cross,
    XCross,
    UTriangle,
    DTriangle,
    LTriangle,
    RTriangle,
    Pentagon,
    Hexagon,
    Star5,
    FullCircle,
    FullRect,
}

impl Marker {
    pub(crate) fn shader_id(self) -> u32 {
        self as u32
    }
}

/// Line dash patterns (in units of linewidth, as in Makie).
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Linestyle {
    #[default]
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
    /// Cumulative boundaries in units of linewidth, starting at 0: `[0, on, off+on, ...]`.
    Custom(Vec<f32>),
}

impl Linestyle {
    /// Makie's cumulative pattern, or `None` for solid.
    pub(crate) fn pattern(&self) -> Option<Vec<f32>> {
        match self {
            Linestyle::Solid => None,
            Linestyle::Dash => Some(vec![0.0, 3.0, 6.0]),
            Linestyle::Dot => Some(vec![0.0, 1.0, 3.0]),
            Linestyle::DashDot => Some(vec![0.0, 3.0, 6.0, 7.0, 10.0]),
            Linestyle::DashDotDot => Some(vec![0.0, 3.0, 6.0, 7.0, 9.0, 10.0, 13.0]),
            Linestyle::Custom(v) => Some(v.clone()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineCap {
    #[default]
    Butt,
    Square,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum JoinStyle {
    #[default]
    Miter,
    Bevel,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum HAlign {
    Left,
    #[default]
    Center,
    Right,
    /// Fraction of the available width (0 = left, 1 = right).
    Frac(f64),
}

impl HAlign {
    pub(crate) fn frac(self) -> f64 {
        match self {
            HAlign::Left => 0.0,
            HAlign::Center => 0.5,
            HAlign::Right => 1.0,
            HAlign::Frac(f) => f,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum VAlign {
    Bottom,
    #[default]
    Center,
    Top,
    /// Fraction of the available height (0 = bottom, 1 = top).
    Frac(f64),
}

impl VAlign {
    pub(crate) fn frac(self) -> f64 {
        match self {
            VAlign::Bottom => 0.0,
            VAlign::Center => 0.5,
            VAlign::Top => 1.0,
            VAlign::Frac(f) => f,
        }
    }
}

/// Histogram normalization (Makie's `normalization`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Normalization {
    #[default]
    None,
    Pdf,
    Density,
    Probability,
}

/// Orientation of bars and bands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Direction {
    X,
    #[default]
    Y,
}

conv_identity!(Marker, Linestyle, LineCap, JoinStyle, HAlign, VAlign, Normalization, Direction);
