//! wgpu backend: one shared device for every window and export, five pipelines (`mesh`, `sprite`,
//! `line`, `glyph`, `field`), painter's order in a single 4x MSAA pass.
//!
//! Portable to WebGPU and WebGL2: no storage buffers, compute or storage textures. Shaders read
//! per-instance data from vertex buffers and large arrays from data textures (`textureLoad`),
//! stay within the WebGL2 limits (8 vertex buffers, 16 attributes, 16 KiB uniform blocks) and
//! are built per target format (a web canvas may be `Rgba8Unorm` or `Bgra8Unorm`).

mod frame;
mod pipelines;
mod renderer;
pub mod testing;

pub use frame::RenderStats;
pub(crate) use renderer::Renderer;

use crate::error::{Error, Result};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The window surface format (non-sRGB: blending happens on sRGB-encoded values, like Cairo and
/// GLMakie). Pipelines exist for any format; this is just the native windows' choice.
pub(crate) const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
/// Offscreen (PNG export) target format: renderable on every backend, WebGL2 included.
pub(crate) const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub(crate) const MSAA: u32 = 4;

pub(crate) struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    layouts: Arc<pipelines::Layouts>,
    /// Pipeline sets, one per target format (built on first use).
    pipelines: Mutex<Vec<(wgpu::TextureFormat, Arc<pipelines::Pipelines>)>>,
    /// Uncaptured wgpu errors (validation and out-of-memory) since creation.
    errors: Arc<AtomicU64>,
}

impl Gpu {
    /// Creates a device on an adapter of `instance` (compatible with `surface`, which WebGL needs).
    /// `limits = None` requests everything the adapter offers.
    pub(crate) async fn create(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        limits: Option<wgpu::Limits>,
    ) -> std::result::Result<Gpu, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        let required_limits = limits.unwrap_or_else(|| adapter.limits());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor { label: Some("ezviz"), required_limits, ..Default::default() })
            .await
            .map_err(|e| e.to_string())?;
        let errors = Arc::new(AtomicU64::new(0));
        let counter = errors.clone();
        device.on_uncaptured_error(Arc::new(move |e| {
            counter.fetch_add(1, Ordering::Relaxed);
            log::error!("wgpu: {e}");
        }));
        let layouts = Arc::new(pipelines::Layouts::new(&device));
        Ok(Gpu { instance, adapter, device, queue, layouts, pipelines: Mutex::new(Vec::new()), errors })
    }

    /// The pipeline set drawing into `format` targets.
    pub(crate) fn pipelines(&self, format: wgpu::TextureFormat) -> Arc<pipelines::Pipelines> {
        let mut sets = self.pipelines.lock();
        if let Some((_, p)) = sets.iter().find(|(f, _)| *f == format) {
            return p.clone();
        }
        let p = Arc::new(pipelines::Pipelines::new(&self.device, &self.layouts, format));
        sets.push((format, p.clone()));
        p
    }

    pub(crate) fn layouts(&self) -> Arc<pipelines::Layouts> {
        self.layouts.clone()
    }

    pub(crate) fn max_texture_size(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    /// Uncaptured wgpu errors so far.
    pub(crate) fn error_count(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }
}

/// A wgpu instance for every backend this build supports. On the web it checks for WebGPU and
/// falls back to WebGL2 when the browser has none.
pub(crate) async fn new_instance() -> wgpu::Instance {
    let desc = wgpu::InstanceDescriptor::new_without_display_handle();
    #[cfg(target_arch = "wasm32")]
    {
        wgpu::util::new_instance_with_webgpu_detection(desc).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wgpu::Instance::new(desc)
    }
}

#[cfg(not(target_arch = "wasm32"))]
static GPU: std::sync::OnceLock<std::result::Result<Arc<Gpu>, String>> = std::sync::OnceLock::new();

#[cfg(target_arch = "wasm32")]
thread_local! {
    static GPU: std::cell::RefCell<Option<Arc<Gpu>>> = const { std::cell::RefCell::new(None) };
}

/// The shared GPU context, created on first use without blocking (the browser path). `surface`
/// (with the instance it was created from) picks an adapter able to present to it; WebGL2 has no
/// adapter without one. Later calls return the context created first.
pub(crate) async fn gpu_async(surface: Option<(wgpu::Instance, &wgpu::Surface<'_>)>) -> Result<Arc<Gpu>> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(r) = GPU.get() {
        return r.clone().map_err(Error::NoGpuAdapter);
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(g) = GPU.with(|g| g.borrow().clone()) {
        return Ok(g);
    }
    let (instance, surface) = match surface {
        Some((i, s)) => (i, Some(s)),
        None => (new_instance().await, None),
    };
    let made = Gpu::create(instance, surface, None).await.map(Arc::new);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = GPU.set(made);
        GPU.get().cloned().unwrap_or_else(|| Err("GPU initialization raced".into())).map_err(Error::NoGpuAdapter)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let g = made.map_err(Error::NoGpuAdapter)?;
        Ok(GPU.with(|slot| slot.borrow_mut().get_or_insert(g).clone()))
    }
}

/// The shared GPU context (created on first use; blocks until the device exists).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn gpu() -> Result<Arc<Gpu>> {
    GPU.get_or_init(|| pollster::block_on(async { Gpu::create(new_instance().await, None, None).await.map(Arc::new) }))
        .clone()
        .map_err(Error::NoGpuAdapter)
}

/// The shared GPU context, if [`gpu_async`] already created it (the browser can't block).
#[cfg(target_arch = "wasm32")]
pub(crate) fn gpu() -> Result<Arc<Gpu>> {
    GPU.with(|g| g.borrow().clone())
        .ok_or_else(|| Error::NoGpuAdapter("the GPU context is created asynchronously in the browser".into()))
}
