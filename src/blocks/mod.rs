//! Layout blocks: things that occupy grid cells (Makie's `Block`s).
//!
//! `Axis` is special-cased by the scene builder (limits, ticks, plots). Every other block type
//! implements [`BlockImpl`] and registers with one line in `block_kinds!` below:
//! - `layout` reports its layout request (size, autosize, protrusions, alignment),
//! - `emit` draws it into its solved rectangle,
//! - `inside_axis` places it over an axis instead of in a grid cell (e.g. `axislegend`).

pub(crate) mod axis;
pub(crate) mod label;
pub(crate) mod legend;

pub use axis::{Axis, linkaxes, linkxaxes, linkyaxes};
pub use label::Label;
pub use legend::{Legend, LegendEntry, LegendSource, Orientation, PlotRef, Pos, axislegend};

use crate::figure::{BlockId, FigState};
use crate::layout::{AlignMode, BlockSize, Protrusion};
use crate::scene::AxisFrame;
use crate::scene::drawlist::{Emitter, Rect};
use crate::theme::Globals;

/// What a block sees while it is laid out and drawn.
pub(crate) struct BlockCtx<'a> {
    pub st: &'a FigState,
    pub g: &'a Globals,
    /// Every axis of the figure; during `emit` their rectangles and limits are final.
    pub axes: &'a [AxisFrame],
    pub id: BlockId,
}

/// A block's layout request (its grid span and side come from its placement).
#[derive(Clone, Debug)]
pub(crate) struct BlockLayout {
    pub protrusion: Protrusion,
    pub width: BlockSize,
    pub height: BlockSize,
    /// Preferred size (used by `BlockSize::Auto`).
    pub autosize: [Option<f64>; 2],
    pub tellwidth: bool,
    pub tellheight: bool,
    /// 0 = left .. 1 = right.
    pub halign: f64,
    /// 0 = bottom .. 1 = top.
    pub valign: f64,
    pub alignmode: AlignMode,
}

impl Default for BlockLayout {
    fn default() -> Self {
        BlockLayout {
            protrusion: Protrusion::default(),
            width: BlockSize::Auto,
            height: BlockSize::Auto,
            autosize: [None, None],
            tellwidth: true,
            tellheight: true,
            halign: 0.5,
            valign: 0.5,
            alignmode: AlignMode::Inside,
        }
    }
}

/// What every non-Axis block type implements.
pub(crate) trait BlockImpl {
    fn layout(&self, ctx: &BlockCtx<'_>) -> BlockLayout;
    /// Draws the block into `rect`, its solved main area (figure units, y down).
    fn emit(&self, ctx: &BlockCtx<'_>, em: &mut Emitter, rect: Rect);
    /// Draw over this axis instead of taking a grid cell (Makie's `axislegend`).
    fn inside_axis(&self) -> Option<BlockId> {
        None
    }
}

/// Declares the block registry: `Block` (with the special `Axis` variant) and dispatch.
macro_rules! block_kinds {
    ($($V:ident($T:ty)),* $(,)?) => {
        /// A block's state.
        #[derive(Clone, Debug)]
        pub(crate) enum Block {
            Axis(Box<axis::AxisState>),
            $($V(Box<$T>)),*
        }

        impl Block {
            /// The block's implementation (`None` for Axis, which the scene builder handles).
            pub(crate) fn imp(&self) -> Option<&dyn BlockImpl> {
                match self {
                    Block::Axis(_) => None,
                    $(Block::$V(b) => Some(&**b)),*
                }
            }
        }
    };
}

block_kinds! {
    Label(label::LabelState),
    Legend(legend::LegendState),
}

impl Block {
    pub(crate) fn as_axis(&self) -> Option<&axis::AxisState> {
        match self {
            Block::Axis(a) => Some(a),
            _ => None,
        }
    }
    pub(crate) fn as_axis_mut(&mut self) -> Option<&mut axis::AxisState> {
        match self {
            Block::Axis(a) => Some(a),
            _ => None,
        }
    }
}

/// Generates the methods every non-Axis block handle shares (`figure`, `delete`, Debug, PartialEq).
macro_rules! block_common {
    ($Handle:ident, $Variant:ident, $State:ty) => {
        impl $Handle {
            pub(crate) fn with_state<R>(&self, dirty: u8, f: impl FnOnce(&mut $State) -> R) -> Option<R> {
                let r = self.sh.update(dirty, |st| match st.block_mut(self.id) {
                    Some($crate::blocks::Block::$Variant(b)) => Some(f(b)),
                    _ => None,
                });
                if r.is_none() {
                    $crate::warn_once(concat!("setter called on a ", stringify!($Handle), " that no longer exists"));
                }
                r
            }

            /// The figure containing this block.
            pub fn figure(&self) -> $crate::Figure {
                $crate::Figure { sh: self.sh.clone() }
            }

            /// Removes the block from its figure.
            pub fn delete(&self) {
                let id = self.id;
                self.sh.update($crate::figure::Dirty::LAYOUT, |st| {
                    if st.block(id).is_some() {
                        st.blocks[id.index as usize] = None;
                    }
                });
            }
        }

        impl PartialEq for $Handle {
            fn eq(&self, other: &Self) -> bool {
                std::sync::Arc::ptr_eq(&self.sh, &other.sh) && self.id == other.id
            }
        }

        impl std::fmt::Debug for $Handle {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!(stringify!($Handle), "#{}"), self.id.index)
            }
        }
    };
}
pub(crate) use block_common;
