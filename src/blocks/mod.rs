//! Layout blocks: things that occupy grid cells.

pub(crate) mod axis;

pub use axis::{Axis, linkaxes, linkxaxes, linkyaxes};

/// A block's state.
#[derive(Clone, Debug)]
pub(crate) enum Block {
    Axis(Box<axis::AxisState>),
}

impl Block {
    pub(crate) fn as_axis(&self) -> Option<&axis::AxisState> {
        match self {
            Block::Axis(a) => Some(a),
        }
    }
    pub(crate) fn as_axis_mut(&mut self) -> Option<&mut axis::AxisState> {
        match self {
            Block::Axis(a) => Some(a),
        }
    }
}
