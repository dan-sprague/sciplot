//! Hooks for tests and benchmarks (not part of the stable API).

use crate::error::Result;
use crate::figure::Figure;
pub use crate::render::gpu::RenderStats;
use crate::render::gpu::{Renderer, gpu};

/// A persistent headless render context, like a window without a window.
pub struct Offscreen {
    r: Renderer,
    ppu: f64,
}

impl Offscreen {
    pub fn new(ppu: f64) -> Result<Offscreen> {
        Ok(Offscreen { r: Renderer::new(gpu()?), ppu })
    }

    /// Renders one frame of `fig` and waits for the GPU. Returns the frame's upload stats.
    pub fn frame(&mut self, fig: &Figure) -> Result<RenderStats> {
        let st = fig.sh.snapshot();
        let (dl, _) = crate::scene::build(&st, None, &mut self.r.scene);
        self.r.render_rgba(&dl, self.ppu)?;
        Ok(self.r.stats)
    }
}

/// Interactive-style limits change (what pan/zoom does), in data coordinates.
pub fn set_interactive_limits(ax: &crate::Axis, lims: [f64; 4]) {
    ax.with_state(crate::figure::Dirty::LIMITS, |a| a.interactive = Some(lims));
}

/// The grid layout solver, for the GridLayoutBase fixture tests (`tests/layout.rs`).
pub mod layout {
    pub use crate::layout::{AlignMode, BBox, BlockSize, Content, Gap, Grid, LayoutItem, MixedSide, Protrusion};
}
