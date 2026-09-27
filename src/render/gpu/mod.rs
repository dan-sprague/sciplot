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
            .request_device(&wgpu::DeviceDescriptor { label: Some("sciplot"), required_limits, ..Default::default() })
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

    /// The backend the device runs on (`BrowserWebGpu` or `Gl` in the browser).
    pub(crate) fn backend(&self) -> wgpu::Backend {
        self.adapter.get_info().backend
    }

    pub(crate) fn max_texture_size(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    /// Uncaptured wgpu errors so far.
    pub(crate) fn error_count(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }
}

/// The format a window surface is configured with: [`TARGET_FORMAT`] when the surface offers it,
/// else the first non-sRGB 8-bit format it lists. WebGL2 canvases list `Rgba8UnormSrgb` first and
/// have no BGRA, so never take `formats[0]`.
pub(crate) fn surface_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat::{Bgra8Unorm, Rgba8Unorm};
    if formats.contains(&TARGET_FORMAT) {
        return Some(TARGET_FORMAT);
    }
    formats.iter().copied().find(|f| matches!(f, Rgba8Unorm | Bgra8Unorm))
}

/// A wgpu instance for every backend this build supports. On the web it checks for WebGPU and
/// falls back to WebGL2 when the browser has none.
pub(crate) async fn new_instance() -> wgpu::Instance {
    new_instance_with(wgpu::Backends::all()).await
}

/// A wgpu instance restricted to `backends`. On the web WebGPU is kept only if the browser really
/// provides an adapter (`navigator.gpu` may exist without one).
pub(crate) async fn new_instance_with(backends: wgpu::Backends) -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
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
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (instance, surface) = match surface {
            Some((i, s)) => (i, Some(s)),
            None => (new_instance().await, None),
        };
        let made = Gpu::create(instance, surface, None).await.map(Arc::new);
        let _ = GPU.set(made);
        GPU.get().cloned().unwrap_or_else(|| Err("GPU initialization raced".into())).map_err(Error::NoGpuAdapter)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let made = match surface {
            Some((i, s)) => Gpu::create(i, Some(s), None).await,
            None => web_offscreen_gpu().await,
        };
        let g = Arc::new(made.map_err(Error::NoGpuAdapter)?);
        Ok(GPU.with(|slot| slot.borrow_mut().get_or_insert(g).clone()))
    }
}

/// Browser: whether the page URL forces WebGL2 (`?backend=gl`).
#[cfg(target_arch = "wasm32")]
pub(crate) fn web_force_gl() -> bool {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .is_some_and(|q| q.trim_start_matches('?').split('&').any(|kv| matches!(kv, "backend=gl" | "backend=webgl2")))
}

/// The backends a new browser context may use.
#[cfg(target_arch = "wasm32")]
fn web_backends(force_gl: bool) -> wgpu::Backends {
    if force_gl { wgpu::Backends::GL } else { wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL }
}

/// A context for offscreen rendering in the browser. WebGPU needs no surface; WebGL2 has no
/// adapter without a canvas, so it gets a detached one.
#[cfg(target_arch = "wasm32")]
async fn web_offscreen_gpu() -> std::result::Result<Gpu, String> {
    use wasm_bindgen::JsCast;
    let instance = new_instance_with(web_backends(web_force_gl())).await;
    let first = match Gpu::create(instance.clone(), None, None).await {
        Ok(g) => return Ok(g),
        Err(e) => e,
    };
    let canvas = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.create_element("canvas").ok())
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .ok_or(first)?;
    let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas)).map_err(|e| e.to_string())?;
    Gpu::create(instance, Some(&surface), None).await
}

/// The shared GPU context (created on first use; blocks until the device exists).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn gpu() -> Result<Arc<Gpu>> {
    GPU.get_or_init(|| pollster::block_on(async { Gpu::create(new_instance().await, None, None).await.map(Arc::new) }))
        .clone()
        .map_err(Error::NoGpuAdapter)
}

/// A GPU context and a surface presenting to `target` (a winit window's canvas).
///
/// On WebGPU one device serves every canvas, so the shared context is reused. On WebGL2 a device
/// belongs to the GL context of one canvas, so every canvas gets a context of its own (the first
/// one also becomes the shared context that exports use). `?backend=gl` in the page URL
/// restricts canvases to WebGL2. The backend is chosen before the surface exists: a canvas that
/// handed out a `webgpu` context can't give a `webgl2` one.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn gpu_for_surface(
    target: impl Into<wgpu::SurfaceTarget<'static>>,
) -> Result<(Arc<Gpu>, wgpu::Surface<'static>)> {
    let force_gl = web_force_gl();
    let shared = GPU.with(|g| g.borrow().clone());
    if let Some(g) = shared.filter(|g| !force_gl && g.backend() == wgpu::Backend::BrowserWebGpu) {
        let surface = g.instance.create_surface(target).map_err(|e| Error::Gpu(e.to_string()))?;
        return Ok((g, surface));
    }
    let instance = new_instance_with(web_backends(force_gl)).await;
    let surface = instance.create_surface(target).map_err(|e| Error::Gpu(e.to_string()))?;
    let gpu = Arc::new(Gpu::create(instance, Some(&surface), None).await.map_err(Error::NoGpuAdapter)?);
    GPU.with(|slot| {
        slot.borrow_mut().get_or_insert_with(|| gpu.clone());
    });
    Ok((gpu, surface))
}

/// The shared GPU context, if [`gpu_async`] already created it (the browser can't block).
#[cfg(target_arch = "wasm32")]
pub(crate) fn gpu() -> Result<Arc<Gpu>> {
    GPU.with(|g| g.borrow().clone())
        .ok_or_else(|| Error::NoGpuAdapter("the GPU context is created asynchronously in the browser".into()))
}

#[cfg(test)]
mod tests {
    use super::surface_format;
    use wgpu::TextureFormat::*;

    #[test]
    fn surface_format_is_never_srgb() {
        // Native Metal / Chrome WebGPU: BGRA preferred.
        assert_eq!(surface_format(&[Bgra8UnormSrgb, Bgra8Unorm, Rgba8Unorm]), Some(Bgra8Unorm));
        assert_eq!(surface_format(&[Bgra8Unorm, Rgba8Unorm, Rgba16Float]), Some(Bgra8Unorm));
        // WebGL2 lists sRGB first and has no BGRA.
        assert_eq!(surface_format(&[Rgba8UnormSrgb, Rgba8Unorm, Rgba16Float]), Some(Rgba8Unorm));
        assert_eq!(surface_format(&[Rgba8UnormSrgb, Rgba16Float]), None);
    }
}
