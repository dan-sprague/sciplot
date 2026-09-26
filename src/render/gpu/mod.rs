//! wgpu backend: one shared device for every window and export, four pipelines
//! (`mesh`, `sprite`, and later `line`, `field`), painter's order in a single 4x MSAA pass.

mod frame;
mod pipelines;
mod renderer;

pub use frame::RenderStats;
pub(crate) use renderer::Renderer;

use crate::error::{Error, Result};
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};

/// Every target (window surface and offscreen) uses this format so one pipeline set serves all.
/// Non-sRGB: blending happens on sRGB-encoded values, like Cairo and GLMakie.
pub(crate) const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
pub(crate) const MSAA: u32 = 4;

pub(crate) struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pipelines: Mutex<Option<Arc<pipelines::Pipelines>>>,
}

impl Gpu {
    fn new() -> std::result::Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ezviz"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        device.on_uncaptured_error(Arc::new(|e| log::error!("wgpu: {e}")));
        Ok(Gpu { instance, adapter, device, queue, pipelines: Mutex::new(None) })
    }

    pub(crate) fn pipelines(&self) -> Arc<pipelines::Pipelines> {
        self.pipelines.lock().get_or_insert_with(|| Arc::new(pipelines::Pipelines::new(&self.device))).clone()
    }

    pub(crate) fn max_texture_size(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }
}

/// The shared GPU context (created on first use).
pub(crate) fn gpu() -> Result<Arc<Gpu>> {
    static GPU: OnceLock<std::result::Result<Arc<Gpu>, String>> = OnceLock::new();
    GPU.get_or_init(|| Gpu::new().map(Arc::new)).clone().map_err(Error::NoGpuAdapter)
}
