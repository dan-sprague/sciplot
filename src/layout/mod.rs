//! Grid layout: a port of GridLayoutBase's solver, which places blocks so that axes line up by
//! their spines.
//!
//! Every block reports a [`LayoutItem`]: its grid span and [`Side`], the decoration room it needs
//! outside its main area (protrusions), its size attributes and the size it would like (autosize),
//! whether that size should determine its column width / row height (`tellwidth`/`tellheight`),
//! and its alignment inside a larger cell. [`solve`] returns each block's main area (for an Axis:
//! the rectangle inside the spines, rounded to integer units like Makie's scene viewports).
//!
//! The solver works on a tree ([`Grid`] holding [`Content`]), so nested grid layouts are supported;
//! the figure's root grid uses `Outside(figure_padding)` like Makie. Inside this module coordinates
//! are Makie's: y up, origin at the bottom-left of the figure ([`BBox`]); [`solve`] converts its
//! results to the scene's y-down [`Rect`]s.
//!
//! Provenance: the types (`Protrusion`, `BlockSize`, `AlignMode`, `MixedSide`, `Gap`) and the
//! `LayoutItem` defaults are adapted from GridLayoutBase 0.11.3 `src/types.jl` and
//! `src/layoutobservables.jl`. `BBox::round` is ported from Makie 0.24.14
//! `src/makielayout/helpers.jl` (`round_to_IRect2D`); the root grid's `Outside(figure_padding)` and
//! `tight_size` follow Makie 0.24.14 `src/figures.jl` (`Figure`, `resize_to_layout!`). MIT
//! licensed; see THIRD_PARTY_NOTICES.md.

mod grid;

pub use grid::{Content, Grid};

use crate::figure::{GridSize, GridSpec, Side};
use crate::scene::drawlist::Rect;

/// Sizes per rectangle side in units: protrusions (decoration room outside a block's main area)
/// or paddings. Makie's `RectSides` with fields in its order (left, right, bottom, top).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Protrusion {
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
    pub top: f64,
}

impl Protrusion {
    /// The same value on all four sides.
    pub fn all(v: f64) -> Protrusion {
        Protrusion { left: v, right: v, bottom: v, top: v }
    }

    /// From `[left, right, bottom, top]`.
    pub fn from_array([left, right, bottom, top]: [f64; 4]) -> Protrusion {
        Protrusion { left, right, bottom, top }
    }
}

/// An axis-aligned box in Makie's layout coordinates (y up): left, right, bottom, top.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BBox {
    pub l: f64,
    pub r: f64,
    pub b: f64,
    pub t: f64,
}

impl BBox {
    /// From an origin (bottom-left corner) and a size, like Makie's `Rect2f`.
    pub fn from_origin_size(x: f64, y: f64, w: f64, h: f64) -> BBox {
        BBox { l: x, r: x + w, b: y, t: y + h }
    }

    pub fn width(&self) -> f64 {
        self.r - self.l
    }

    pub fn height(&self) -> f64 {
        self.t - self.b
    }

    /// Makie's `round_to_IRect2D` (how an Axis turns its layout bbox into its scene viewport):
    /// both corners are rounded to the nearest integer, ties to even, like Julia's `round`.
    pub fn round(self) -> BBox {
        let f = |v: f64| if v.is_finite() { v.round_ties_even() } else { 0.0 };
        BBox { l: f(self.l), r: f(self.r), b: f(self.b), t: f(self.t) }
    }

    /// The same box in the scene's y-down figure units, for a figure `fig_h` units high.
    pub(crate) fn to_rect(self, fig_h: f64) -> Rect {
        Rect::new(self.l, fig_h - self.t, self.width(), self.height())
    }
}

/// A block's `width` or `height` attribute (Makie's `SizeAttribute`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum BlockSize {
    /// Fill the suggested cell (Makie `nothing`; the Axis default).
    #[default]
    Fill,
    /// A fixed size in units (Makie `Fixed(x)` or a bare number).
    Fixed(f64),
    /// A fraction of the suggested cell (never determines the column/row size).
    Relative(f64),
    /// The block's own preferred size (its autosize), e.g. a Label's text extent.
    Auto,
}

impl From<Option<f64>> for BlockSize {
    /// `None` fills the cell, `Some(v)` is fixed (how Axis `width`/`height` are stored).
    fn from(v: Option<f64>) -> BlockSize {
        v.map_or(BlockSize::Fill, BlockSize::Fixed)
    }
}

/// How a block or grid treats its protrusions when fitting into its bounding box (Makie
/// `AlignMode`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum AlignMode {
    /// The main area fills the bbox and protrusions stick out of it (the block default).
    #[default]
    Inside,
    /// Protrusions plus this padding fit inside the bbox (the figure's root layout).
    Outside(Protrusion),
    /// Per-side choice.
    Mixed { left: MixedSide, right: MixedSide, bottom: MixedSide, top: MixedSide },
}

/// One side of [`AlignMode::Mixed`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum MixedSide {
    /// Like [`AlignMode::Inside`] on this side (Makie `nothing`).
    #[default]
    Inside,
    /// Like [`AlignMode::Outside`] with this padding (Makie: a number).
    Pad(f64),
    /// Reports this protrusion instead of the real one (Makie `Protrusion(p)`).
    Protrusion(f64),
}

/// A gap between two columns or rows, in addition to the protrusions sticking into it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gap {
    Fixed(f64),
    /// A fraction of the space left after protrusions.
    Relative(f64),
}

/// One block's layout request.
#[derive(Clone, Debug)]
pub struct LayoutItem {
    /// 1-based inclusive row span.
    pub rows: (i32, i32),
    /// 1-based inclusive column span.
    pub cols: (i32, i32),
    /// `Inner` for normal content; the other sides place the block in the protrusion strip next to
    /// the cell (e.g. a panel label at `TopLeft`).
    pub side: Side,
    pub protrusion: Protrusion,
    pub width: BlockSize,
    pub height: BlockSize,
    /// The block's preferred size, used by [`BlockSize::Auto`] (Makie `autosize`).
    pub autosize: [Option<f64>; 2],
    /// Whether the block's width/height should determine its column width/row height.
    pub tellwidth: bool,
    pub tellheight: bool,
    /// Where a smaller block sits in its cell: 0 = left, 0.5 = center, 1 = right.
    pub halign: f64,
    /// Where a smaller block sits in its cell: 0 = bottom, 0.5 = center, 1 = top (Makie's).
    pub valign: f64,
    pub alignmode: AlignMode,
    /// Round the solved area to integer units (an Axis's scene viewport).
    pub round: bool,
}

impl Default for LayoutItem {
    fn default() -> Self {
        LayoutItem {
            rows: (1, 1),
            cols: (1, 1),
            side: Side::Inner,
            protrusion: Protrusion::default(),
            width: BlockSize::Fill,
            height: BlockSize::Fill,
            autosize: [None, None],
            tellwidth: true,
            tellheight: true,
            halign: 0.5,
            valign: 0.5,
            alignmode: AlignMode::Inside,
            round: false,
        }
    }
}

/// The figure's root grid for `items`: its size follows the items' spans, column/row sizes and gaps
/// come from `spec` (falling back to the theme's `colgap`/`rowgap`), and it is aligned
/// `Outside(padding)` with `padding = [left, right, bottom, top]`.
pub(crate) fn root_grid(items: &[LayoutItem], spec: &GridSpec, padding: [f64; 4], colgap: f64, rowgap: f64) -> Grid {
    let nrows = items.iter().map(|i| i.rows.1).max().unwrap_or(1).max(1) as usize;
    let ncols = items.iter().map(|i| i.cols.1).max().unwrap_or(1).max(1) as usize;
    let sizes = |n: usize, specs: &[(i32, GridSize)]| {
        let mut v = vec![GridSize::Auto; n];
        for &(i, s) in specs {
            if let Some(slot) = usize::try_from(i - 1).ok().and_then(|i| v.get_mut(i)) {
                *slot = s;
            }
        }
        v
    };
    let gaps = |n: usize, all: f64, specs: &[(i32, f64)]| {
        let mut v = vec![Gap::Fixed(all); n.saturating_sub(1)];
        for &(i, g) in specs {
            if let Some(slot) = usize::try_from(i - 1).ok().and_then(|i| v.get_mut(i)) {
                *slot = Gap::Fixed(g);
            }
        }
        v
    };
    Grid {
        rowsizes: sizes(nrows, &spec.rowsizes),
        colsizes: sizes(ncols, &spec.colsizes),
        rowgaps: gaps(nrows, spec.rowgap.unwrap_or(rowgap), &spec.rowgaps),
        colgaps: gaps(ncols, spec.colgap.unwrap_or(colgap), &spec.colgaps),
        alignmode: AlignMode::Outside(Protrusion::from_array(padding)),
        content: items.iter().cloned().map(Content::Block).collect(),
        ..Grid::new(nrows, ncols)
    }
}

/// The solved main area for each item, in figure units (y down), in input order.
pub(crate) fn solve(
    items: &[LayoutItem],
    spec: &GridSpec,
    size: [f64; 2],
    padding: [f64; 4],
    colgap: f64,
    rowgap: f64,
) -> Vec<Rect> {
    let grid = root_grid(items, spec, padding, colgap, rowgap);
    grid.solve_root(size)
        .into_iter()
        .zip(items)
        .map(|(b, it)| if it.round { b.round() } else { b }.to_rect(size[1]))
        .collect()
}

/// The figure size that fits the layout exactly (Makie `resize_to_layout!`): with Fixed or Aspect
/// columns/rows or fixed-size blocks, the solved grid can be smaller or larger than the figure.
/// Rounded to integer units, as Makie's scene viewport is.
pub(crate) fn tight_size(
    items: &[LayoutItem],
    spec: &GridSpec,
    size: [f64; 2],
    padding: [f64; 4],
    colgap: f64,
    rowgap: f64,
) -> [f64; 2] {
    let b = root_grid(items, spec, padding, colgap, rowgap).tight_bbox(size);
    [b.width().round_ties_even().max(1.0), b.height().round_ties_even().max(1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(rows: (i32, i32), cols: (i32, i32), p: [f64; 4]) -> LayoutItem {
        LayoutItem { rows, cols, protrusion: Protrusion::from_array(p), round: true, ..Default::default() }
    }

    /// docs/research/makie-algorithms.md §4.7: the default Axis viewport is (74, 59) 510×355 (y up),
    /// i.e. top 450 - 414 = 36 in y-down units.
    #[test]
    fn worked_example() {
        let it = axis((1, 1), (1, 1), [57.95, 0.0, 42.62, 20.31]);
        let r = solve(&[it], &GridSpec::default(), [600.0, 450.0], [16.0; 4], 18.0, 18.0);
        assert_eq!(r[0], Rect::new(74.0, 36.0, 510.0, 355.0));
    }

    #[test]
    fn empty_and_degenerate_inputs_do_not_panic() {
        let spec = GridSpec {
            colsizes: vec![(1, GridSize::Aspect(1, 1.0)), (7, GridSize::Fixed(10.0))],
            rowsizes: vec![(1, GridSize::Aspect(1, 1.0)), (0, GridSize::Auto)],
            ..Default::default()
        };
        assert!(solve(&[], &spec, [600.0, 450.0], [16.0; 4], 18.0, 18.0).is_empty());
        let it = axis((1, 2), (1, 3), [1e9, 0.0, 0.0, f64::NAN]);
        let r = solve(std::slice::from_ref(&it), &spec, [0.0, 0.0], [16.0; 4], 18.0, 18.0);
        assert_eq!(r.len(), 1);
        let s = tight_size(&[it], &spec, [10.0, 10.0], [16.0; 4], 18.0, 18.0);
        assert!(s[0] >= 1.0 && s[1] >= 1.0);
    }

    #[test]
    fn tight_size_of_fixed_columns() {
        let spec = GridSpec {
            colsizes: vec![(1, GridSize::Fixed(200.0))],
            rowsizes: vec![(1, GridSize::Fixed(100.0))],
            ..Default::default()
        };
        let it = axis((1, 1), (1, 1), [30.0, 0.0, 20.0, 10.0]);
        let s = tight_size(&[it], &spec, [600.0, 450.0], [16.0; 4], 18.0, 18.0);
        assert_eq!(s, [200.0 + 30.0 + 32.0, 100.0 + 30.0 + 32.0]);
    }
}
