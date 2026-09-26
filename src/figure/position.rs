//! Grid positions: Makie's `fig[row, col]`, 1-based and inclusive.

use super::{Figure, Placement};

/// A span of rows or columns. Build it with [`IntoSpan`]: `2`, `1..=3`, `..` or [`Prepend`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    /// Inclusive, 1-based.
    Range(i32, i32),
    /// Every existing row/column (`..`).
    Full,
    /// A new first row/column; existing content shifts down/right.
    Prepend,
}

/// Inserts a new first row or column: `fig.at(Prepend, ..)` for a super-title.
#[derive(Clone, Copy, Debug)]
pub struct Prepend;

/// Row/column specifiers for [`Figure::at`].
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a grid row/column",
    label = "grid positions are 1-based and inclusive like Makie: use `2`, `1..=2`, `..` or `Prepend`",
    note = "half-open ranges like `1..2` are not accepted; write `1..=2`"
)]
pub trait IntoSpan {
    #[track_caller]
    fn into_span(self) -> Span;
}

#[track_caller]
fn one(i: i64) -> Span {
    assert!(
        i >= 1,
        "grid positions are 1-based like Makie: got {i}; use fig.at(Prepend, ..) to insert a new first row"
    );
    Span::Range(i as i32, i as i32)
}

#[track_caller]
fn range(a: i64, b: i64) -> Span {
    assert!(a >= 1 && b >= 1, "grid positions are 1-based like Makie: got {a}..={b}");
    assert!(a <= b, "empty grid span {a}..={b}");
    Span::Range(a as i32, b as i32)
}

impl IntoSpan for i32 {
    #[track_caller]
    fn into_span(self) -> Span {
        one(self as i64)
    }
}
impl IntoSpan for usize {
    #[track_caller]
    fn into_span(self) -> Span {
        one(self as i64)
    }
}
impl IntoSpan for std::ops::RangeInclusive<i32> {
    #[track_caller]
    fn into_span(self) -> Span {
        range(*self.start() as i64, *self.end() as i64)
    }
}
impl IntoSpan for std::ops::RangeInclusive<usize> {
    #[track_caller]
    fn into_span(self) -> Span {
        range(*self.start() as i64, *self.end() as i64)
    }
}
impl IntoSpan for std::ops::RangeFull {
    fn into_span(self) -> Span {
        Span::Full
    }
}
impl IntoSpan for Prepend {
    fn into_span(self) -> Span {
        Span::Prepend
    }
}

/// Protrusion sides for placing labels next to a cell (Makie's `TopLeft()`, etc.).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Side {
    #[default]
    Inner,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// A cell (or span of cells) in a figure's grid. Blocks are created at a position:
/// `Axis::new(fig.at(1, 1))`, `Colorbar::new(fig.at(1, 2), &hm)`.
#[derive(Clone, Debug)]
pub struct GridPosition {
    pub(crate) fig: Figure,
    pub(crate) rows: Span,
    pub(crate) cols: Span,
    pub(crate) side: Side,
}

impl GridPosition {
    pub(crate) fn new(fig: Figure, rows: Span, cols: Span) -> Self {
        GridPosition { fig, rows, cols, side: Side::Inner }
    }

    /// The side protrusion of this cell: `fig.at(1, 1).side(Side::TopLeft)` for panel labels.
    pub fn side(&self, s: Side) -> GridPosition {
        GridPosition { side: s, ..self.clone() }
    }

    /// The figure this position belongs to.
    pub fn figure(&self) -> Figure {
        self.fig.clone()
    }

    /// Resolves spans against the current grid (applying `Prepend` shifts). Must be called with the
    /// state lock held, since `Prepend` moves existing blocks.
    pub(crate) fn resolve(&self, st: &mut super::FigState) -> Placement {
        let (nrows, ncols) = st.grid_extent();
        let rows = match self.rows {
            Span::Range(a, b) => (a, b),
            Span::Full => (1, nrows.max(1)),
            Span::Prepend => {
                for s in st.blocks.iter_mut().flatten() {
                    s.place.rows.0 += 1;
                    s.place.rows.1 += 1;
                }
                (1, 1)
            }
        };
        let (_, ncols2) = st.grid_extent();
        let cols = match self.cols {
            Span::Range(a, b) => (a, b),
            Span::Full => (1, ncols.max(ncols2).max(1)),
            Span::Prepend => {
                for s in st.blocks.iter_mut().flatten() {
                    s.place.cols.0 += 1;
                    s.place.cols.1 += 1;
                }
                (1, 1)
            }
        };
        Placement { rows, cols, side: self.side }
    }
}
