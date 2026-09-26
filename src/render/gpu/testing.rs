//! Hooks for the portability tests (`tests/portability.rs`); not a stable API.

use super::Gpu;
#[cfg(not(target_arch = "wasm32"))]
use super::Renderer;
#[cfg(not(target_arch = "wasm32"))]
use crate::error::{Error, Result};
#[cfg(not(target_arch = "wasm32"))]
use crate::figure::{Figure, RgbaImage};
use std::sync::Arc;

/// A GPU context of its own (not the shared one), optionally restricted to WebGL2's limits.
pub struct GpuContext {
    gpu: Arc<Gpu>,
}

#[cfg(not(target_arch = "wasm32"))]
impl GpuContext {
    /// A context with everything the local adapter offers (what exports and windows use).
    pub fn native() -> Result<GpuContext> {
        Self::with_limits(None)
    }

    /// A context on the local adapter limited to `wgpu::Limits::downlevel_webgl2_defaults()`: no
    /// storage buffers or textures, 8 vertex buffers, 16 attributes, 16 KiB uniform bindings,
    /// 2048 px textures. Pipelines that only work with more fail validation here.
    pub fn webgl2() -> Result<GpuContext> {
        Self::with_limits(Some(wgpu::Limits::downlevel_webgl2_defaults()))
    }

    fn with_limits(limits: Option<wgpu::Limits>) -> Result<GpuContext> {
        let gpu = pollster::block_on(async { Gpu::create(super::new_instance().await, None, limits).await })
            .map_err(Error::NoGpuAdapter)?;
        Ok(GpuContext { gpu: Arc::new(gpu) })
    }

    /// Renders `fig` offscreen at `ppu` into an `Rgba8Unorm` (or, with `bgra`, `Bgra8Unorm`)
    /// target and reads it back as straight-alpha RGBA.
    pub fn render(&self, fig: &Figure, ppu: f64, bgra: bool) -> Result<RgbaImage> {
        let mut r = Renderer::new(self.gpu.clone());
        if bgra {
            r.set_offscreen_format(wgpu::TextureFormat::Bgra8Unorm);
        }
        let st = fig.sh.snapshot();
        let (dl, _) = crate::scene::build(&st, None, &mut r.scene);
        let (width, height, data) = r.render_rgba(&dl, ppu)?;
        Ok(RgbaImage { width, height, data })
    }
}

impl GpuContext {
    /// Uncaptured wgpu errors (validation, out of memory) on this context so far.
    pub fn validation_errors(&self) -> u64 {
        self.gpu.error_count()
    }

    /// Limits the device was created with: `(max_texture_dimension_2d, max_vertex_buffers,
    /// max_storage_buffers_per_shader_stage)`.
    pub fn limits(&self) -> (u32, u32, u32) {
        let l = self.gpu.device.limits();
        (l.max_texture_dimension_2d, l.max_vertex_buffers, l.max_storage_buffers_per_shader_stage)
    }
}

/// Every pipeline's WGSL (with the shared definitions prepended), by pipeline name.
pub fn wgsl_sources() -> Vec<(&'static str, String)> {
    super::pipelines::sources().into_iter().collect()
}
