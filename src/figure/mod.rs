//! `Figure`, grid positions, and the shared state behind every handle.
//!
//! Every handle (`Figure`, `Axis`, plot handles, ...) is an `Arc` to the figure's shared state plus
//! an id, so handles are cheap to clone, `Send + Sync`, and never borrow the figure. All mutation
//! goes through one short-lived lock; rendering snapshots the state (cheap `Arc` clones) and does
//! all heavy work outside the lock.

mod position;
mod save;

pub use position::{GridPosition, IntoSpan, Prepend, Side, Span};
pub(crate) use save::write_png;
pub use save::{RgbaImage, Save};

use crate::blocks::Block;
use crate::plots::PlotSlot;
use crate::theme::{Theme, current_theme};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Dirty classes for a state change (bit flags).
#[allow(non_snake_case, dead_code)]
pub(crate) mod Dirty {
    pub const DATA: u8 = 1;
    pub const STYLE: u8 = 2;
    pub const LIMITS: u8 = 4;
    pub const LAYOUT: u8 = 8;
}

static NEXT_UID: AtomicU64 = AtomicU64::new(1);

/// A process-unique id (figures, plots, render caches).
pub(crate) fn next_uid() -> u64 {
    NEXT_UID.fetch_add(1, Ordering::Relaxed)
}

/// Arena index + generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct BlockId {
    pub index: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PlotId {
    pub index: u32,
    pub generation: u32,
}

/// Where a block sits in its grid.
#[derive(Clone, Debug)]
pub(crate) struct Placement {
    pub rows: (i32, i32),
    pub cols: (i32, i32),
    pub side: Side,
}

#[derive(Clone, Debug)]
pub(crate) struct BlockSlot {
    pub generation: u32,
    pub place: Placement,
    pub block: Block,
}

/// Column/row size specification (Makie `colsize!`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridSize {
    Auto,
    Fixed(f64),
    Relative(f64),
    /// Makie `Aspect(i, ratio)`: this column's width = ratio × height of row `i` (or vice versa).
    Aspect(i32, f64),
    /// Makie `Auto(trydetermine, ratio)`. With `trydetermine = false` the column/row ignores
    /// fixed-size content (e.g. a wide caption) and shares the leftover space like undetermined
    /// Autos, by `ratio`. [`GridSize::Auto`] is `AutoWith { trydetermine: true, ratio: 1.0 }`.
    AutoWith { trydetermine: bool, ratio: f64 },
}

/// The root grid layout's settings.
#[derive(Clone, Debug, Default)]
pub(crate) struct GridSpec {
    pub colsizes: Vec<(i32, GridSize)>,
    pub rowsizes: Vec<(i32, GridSize)>,
    pub colgap: Option<f64>,
    pub rowgap: Option<f64>,
    pub colgaps: Vec<(i32, f64)>,
    pub rowgaps: Vec<(i32, f64)>,
}

/// Everything about a figure. Cloning it is O(#blocks + #plots) with data shared by `Arc`.
#[derive(Clone)]
pub(crate) struct FigState {
    pub theme: Theme,
    pub grid: GridSpec,
    pub blocks: Vec<Option<BlockSlot>>,
    pub plots: Vec<Option<PlotSlot>>,
    /// Bumped by every change.
    pub rev: u64,
    pub batch_depth: u32,
    pub datainspector: bool,
    pub window_title: Option<String>,
}

impl FigState {
    pub(crate) fn block(&self, id: BlockId) -> Option<&Block> {
        match self.blocks.get(id.index as usize) {
            Some(Some(s)) if s.generation == id.generation => Some(&s.block),
            _ => None,
        }
    }
    pub(crate) fn block_mut(&mut self, id: BlockId) -> Option<&mut Block> {
        match self.blocks.get_mut(id.index as usize) {
            Some(Some(s)) if s.generation == id.generation => Some(&mut s.block),
            _ => None,
        }
    }
    pub(crate) fn plot(&self, id: PlotId) -> Option<&PlotSlot> {
        match self.plots.get(id.index as usize) {
            Some(Some(s)) if s.generation == id.generation => Some(s),
            _ => None,
        }
    }
    pub(crate) fn plot_mut(&mut self, id: PlotId) -> Option<&mut PlotSlot> {
        match self.plots.get_mut(id.index as usize) {
            Some(Some(s)) if s.generation == id.generation => Some(s),
            _ => None,
        }
    }

    pub(crate) fn add_block(&mut self, place: Placement, block: Block) -> BlockId {
        let index = self.blocks.len() as u32;
        self.blocks.push(Some(BlockSlot { generation: 0, place, block }));
        BlockId { index, generation: 0 }
    }

    pub(crate) fn add_plot(&mut self, slot: PlotSlot) -> PlotId {
        let index = self.plots.len() as u32;
        let generation = slot.generation;
        self.plots.push(Some(slot));
        PlotId { index, generation }
    }

    /// Blocks with their ids, in insertion order.
    pub(crate) fn iter_blocks(&self) -> impl Iterator<Item = (BlockId, &BlockSlot)> {
        self.blocks
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (BlockId { index: i as u32, generation: s.generation }, s)))
    }

    /// Plots with their ids, in insertion order.
    pub(crate) fn iter_plots(&self) -> impl Iterator<Item = (PlotId, &PlotSlot)> {
        self.plots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (PlotId { index: i as u32, generation: s.generation }, s)))
    }

    /// Grid extent: (nrows, ncols) of the root layout.
    pub(crate) fn grid_extent(&self) -> (i32, i32) {
        self.iter_blocks().fold((0, 0), |(r, c), (_, s)| (r.max(s.place.rows.1), c.max(s.place.cols.1)))
    }
}

type WakeFn = Box<dyn Fn() + Send + Sync>;

/// Wakes attached windows when the state changes (on the false -> true edge only).
pub(crate) struct Waker {
    pending: AtomicBool,
    sinks: Mutex<Vec<(u64, WakeFn)>>,
}

impl Waker {
    fn new() -> Self {
        Waker { pending: AtomicBool::new(false), sinks: Mutex::new(Vec::new()) }
    }
    pub(crate) fn notify(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            for (_, f) in self.sinks.lock().iter() {
                f();
            }
        }
    }
    /// Called by a renderer before it snapshots, so later changes wake it again.
    pub(crate) fn clear(&self) {
        self.pending.store(false, Ordering::Release);
    }
    #[allow(dead_code)]
    pub(crate) fn attach(&self, id: u64, f: WakeFn) {
        self.sinks.lock().push((id, f));
    }
    #[allow(dead_code)]
    pub(crate) fn detach(&self, id: u64) {
        self.sinks.lock().retain(|(i, _)| *i != id);
    }
}

pub(crate) struct FigShared {
    pub(crate) state: Mutex<FigState>,
    pub(crate) wake: Waker,
    pub(crate) uid: u64,
}

impl FigShared {
    /// Applies `f` under the lock, bumps the revision, and wakes renderers unless a batch is open.
    pub(crate) fn update<R>(&self, _dirty: u8, f: impl FnOnce(&mut FigState) -> R) -> R {
        let (r, notify) = {
            let mut st = self.state.lock();
            let r = f(&mut st);
            st.rev += 1;
            (r, st.batch_depth == 0)
        };
        if notify {
            self.wake.notify();
        }
        r
    }

    pub(crate) fn snapshot(&self) -> FigState {
        self.state.lock().clone()
    }
}

/// A figure: the root of a Makie-style layout. Create one with [`Figure::new`] or [`Figure!`].
///
/// ```
/// use ezviz::prelude::*;
/// let fig = Figure::new().size((800, 600));
/// let ax = Axis::new(fig.at(1, 1)).title("hello");
/// ax.scatter([1.0, 2.0, 3.0], [1.0, 4.0, 9.0]);
/// ```
#[derive(Clone)]
pub struct Figure {
    pub(crate) sh: Arc<FigShared>,
}

impl PartialEq for Figure {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.sh, &other.sh)
    }
}

impl Default for Figure {
    fn default() -> Self {
        Figure::new()
    }
}

impl std::fmt::Debug for Figure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Figure#{}", self.sh.uid)
    }
}

impl Figure {
    /// A new figure using the current theme (600 × 450 units by default).
    pub fn new() -> Figure {
        let st = FigState {
            theme: current_theme(),
            grid: GridSpec::default(),
            blocks: Vec::new(),
            plots: Vec::new(),
            rev: 0,
            batch_depth: 0,
            datainspector: true,
            window_title: None,
        };
        Figure { sh: Arc::new(FigShared { state: Mutex::new(st), wake: Waker::new(), uid: next_uid() }) }
    }

    fn set_theme_field(&self, f: impl FnOnce(&mut Theme)) -> Figure {
        self.sh.update(Dirty::LAYOUT, |st| f(&mut st.theme));
        self.clone()
    }

    /// Figure size in units (1 unit = 1 CSS px): `(900, 650)` or `(4.0 * INCH, 3.0 * INCH)`.
    #[track_caller]
    pub fn size(&self, wh: impl crate::attrs::Conv<[f64; 2]>) -> Figure {
        let v = wh.conv();
        self.set_theme_field(|t| t.size = Some(v))
    }
    /// Base font size for everything that inherits it (Makie default 14).
    #[track_caller]
    pub fn fontsize(&self, s: impl crate::attrs::Conv<f64>) -> Figure {
        let v = s.conv();
        self.set_theme_field(|t| t.fontsize = Some(v))
    }
    #[track_caller]
    pub fn backgroundcolor(&self, c: impl crate::attrs::Conv<crate::color::Color>) -> Figure {
        let v = c.conv();
        self.set_theme_field(|t| t.backgroundcolor = Some(v))
    }
    /// One number or `(left, right, bottom, top)`.
    #[track_caller]
    pub fn figure_padding(&self, p: impl crate::attrs::Conv<[f64; 4]>) -> Figure {
        let v = p.conv();
        self.set_theme_field(|t| t.figure_padding = Some(v))
    }
    /// Replaces this figure's theme (attributes resolve against it at render time).
    pub fn theme(&self, t: Theme) -> Figure {
        self.set_theme_field(|th| *th = t)
    }
    /// Hover readout in windows (on by default).
    pub fn datainspector(&self, on: bool) -> Figure {
        self.sh.update(Dirty::STYLE, |st| st.datainspector = on);
        self.clone()
    }
    pub fn window_title(&self, s: impl Into<String>) -> Figure {
        let s = s.into();
        self.sh.update(Dirty::STYLE, |st| st.window_title = Some(s));
        self.clone()
    }

    /// Makie's `fig[row, col]`: a 1-based, inclusive grid position.
    ///
    /// ```
    /// # use ezviz::prelude::*;
    /// let fig = Figure::new();
    /// let top = Axis::new(fig.at(1, 1));
    /// let bottom = Axis::new(fig.at(2, 1..=2));   // spans two columns
    /// ```
    #[track_caller]
    pub fn at(&self, row: impl IntoSpan, col: impl IntoSpan) -> GridPosition {
        GridPosition::new(self.clone(), row.into_span(), col.into_span())
    }

    /// Makes several changes appear atomically in windows (no frame shows half of them).
    pub fn batch<R>(&self, f: impl FnOnce() -> R) -> R {
        struct End<'a>(&'a FigShared);
        impl Drop for End<'_> {
            fn drop(&mut self) {
                let notify = {
                    let mut st = self.0.state.lock();
                    st.batch_depth -= 1;
                    st.batch_depth == 0
                };
                if notify {
                    self.0.wake.notify();
                }
            }
        }
        self.sh.state.lock().batch_depth += 1;
        let _end = End(&self.sh);
        f()
    }

    /// All axes in insertion order.
    pub fn axes(&self) -> Vec<crate::Axis> {
        let st = self.sh.state.lock();
        st.iter_blocks()
            .filter(|(_, s)| matches!(s.block, Block::Axis(_)))
            .map(|(id, _)| crate::Axis { sh: self.sh.clone(), id })
            .collect()
    }

    /// Sets a column's size: `fig.colsize(1, GridSize::Relative(0.3))`.
    pub fn colsize(&self, col: i32, s: GridSize) -> Figure {
        check_index(col, "colsize");
        self.sh.update(Dirty::LAYOUT, |st| {
            st.grid.colsizes.retain(|(c, _)| *c != col);
            st.grid.colsizes.push((col, s));
        });
        self.clone()
    }
    /// Sets a row's size.
    pub fn rowsize(&self, row: i32, s: GridSize) -> Figure {
        check_index(row, "rowsize");
        self.sh.update(Dirty::LAYOUT, |st| {
            st.grid.rowsizes.retain(|(c, _)| *c != row);
            st.grid.rowsizes.push((row, s));
        });
        self.clone()
    }
    /// Gap between all columns (replaces earlier [`Figure::colgap_at`] gaps, like Makie `colgap!`).
    pub fn colgap(&self, g: impl crate::attrs::Conv<f64>) -> Figure {
        let g = g.conv();
        self.sh.update(Dirty::LAYOUT, |st| {
            st.grid.colgap = Some(g);
            st.grid.colgaps.clear();
        });
        self.clone()
    }
    /// Gap between all rows (replaces earlier [`Figure::rowgap_at`] gaps, like Makie `rowgap!`).
    pub fn rowgap(&self, g: impl crate::attrs::Conv<f64>) -> Figure {
        let g = g.conv();
        self.sh.update(Dirty::LAYOUT, |st| {
            st.grid.rowgap = Some(g);
            st.grid.rowgaps.clear();
        });
        self.clone()
    }
    /// Gap between column `i` and column `i + 1` (Makie `colgap!(fig.layout, i, g)`).
    #[track_caller]
    pub fn colgap_at(&self, i: i32, g: impl crate::attrs::Conv<f64>) -> Figure {
        check_index(i, "colgap_at");
        let g = g.conv();
        self.sh.update(Dirty::LAYOUT, |st| {
            st.grid.colgaps.retain(|(c, _)| *c != i);
            st.grid.colgaps.push((i, g));
        });
        self.clone()
    }
    /// Gap between row `i` and row `i + 1` (Makie `rowgap!(fig.layout, i, g)`).
    #[track_caller]
    pub fn rowgap_at(&self, i: i32, g: impl crate::attrs::Conv<f64>) -> Figure {
        check_index(i, "rowgap_at");
        let g = g.conv();
        self.sh.update(Dirty::LAYOUT, |st| {
            st.grid.rowgaps.retain(|(r, _)| *r != i);
            st.grid.rowgaps.push((i, g));
        });
        self.clone()
    }
}

#[track_caller]
pub(crate) fn check_index(i: i32, what: &str) {
    assert!(i >= 1, "{what}: grid positions are 1-based like Makie (got {i})");
}
