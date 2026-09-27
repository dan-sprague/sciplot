//! Hooks for tests and benchmarks (not part of the stable API).

use crate::error::Result;
use crate::figure::Figure;
pub use crate::render::gpu::RenderStats;
use crate::render::gpu::{Renderer, gpu};

/// A persistent headless render context, like a window without a window.
pub struct Offscreen {
    r: Renderer,
    gpu: std::sync::Arc<crate::render::gpu::Gpu>,
    ppu: f64,
}

impl Offscreen {
    pub fn new(ppu: f64) -> Result<Offscreen> {
        let gpu = gpu()?;
        Ok(Offscreen { r: Renderer::new(gpu.clone()), gpu, ppu })
    }

    /// Renders one frame of `fig` and waits for the GPU. Returns the frame's upload stats.
    pub fn frame(&mut self, fig: &Figure) -> Result<RenderStats> {
        let st = fig.sh.snapshot();
        let (dl, _) = crate::scene::build(&st, None, &mut self.r.scene);
        self.r.render_rgba(&dl, self.ppu)?;
        Ok(self.r.stats)
    }

    /// What a window frame costs, without readback: `[snapshot + scene build, encode + GPU]`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn frame_phases(&mut self, fig: &Figure) -> Result<[std::time::Duration; 2]> {
        let t0 = std::time::Instant::now();
        let st = fig.sh.snapshot();
        let (dl, _) = crate::scene::build(&st, None, &mut self.r.scene);
        let t1 = std::time::Instant::now();
        let size = [(dl.size[0] * self.ppu).round() as u32, (dl.size[1] * self.ppu).round() as u32];
        let gpu = self.gpu.clone();
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame_phases"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::render::gpu::TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let cmd = self.r.render(&dl, &view, crate::render::gpu::TARGET_FORMAT, size, self.ppu);
        let idx = gpu.queue.submit([cmd]);
        gpu.device
            .poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None })
            .map_err(|e| crate::error::Error::Gpu(e.to_string()))?;
        Ok([t1 - t0, t1.elapsed()])
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
