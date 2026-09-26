//! Per-frame helpers shared by all pipelines: cached/transient uploads, the uniform ring, and the
//! draw command type.

use super::Gpu;
use super::pipelines::Pipelines;
use crate::color::Color;
use crate::scene::drawlist::{Buf, ColorMapping};
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Uniform blocks are placed at multiples of this (the WebGPU minimum dynamic-offset alignment).
pub(crate) const UNIFORM_ALIGN: usize = 256;

/// One draw call.
pub(crate) struct DrawCmd {
    pub pipeline: wgpu::RenderPipeline,
    /// Bind group 1 (group 0 is the per-frame globals).
    pub bind: wgpu::BindGroup,
    /// Dynamic offset of this draw's uniform block.
    pub offset: u32,
    pub vb: Option<wgpu::Buffer>,
    pub vertices: Range<u32>,
    pub instances: Range<u32>,
}

/// Per-frame upload counters (for perf tests).
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    /// Bytes written to cached (keyed) buffers: plot data re-uploads.
    pub data_bytes: u64,
    /// Bytes written to per-frame buffers (decorations, text).
    pub transient_bytes: u64,
    pub draws: u32,
}

pub(crate) struct Cached {
    pub rev: u64,
    pub buf: wgpu::Buffer,
    pub frame: u64,
}

/// GPU resources that persist across frames of one render context.
pub(crate) struct Resources {
    pub gpu: Arc<Gpu>,
    pub pipes: Arc<Pipelines>,
    pub cache: HashMap<(u64, u8), Cached>,
    pub luts: HashMap<usize, (Arc<Vec<Color>>, wgpu::TextureView, u64)>,
    pub dummy: wgpu::Buffer,
    pub dummy_lut: wgpu::TextureView,
    pub frame: u64,
    pub stats: RenderStats,
    /// The glyph atlas (created on first use).
    pub glyphs: Option<super::pipelines::glyph::GlyphCache>,
}

impl Resources {
    pub fn new(gpu: Arc<Gpu>) -> Resources {
        let pipes = gpu.pipelines();
        let dummy = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("dummy"),
            contents: &[0u8; 16],
            usage: wgpu::BufferUsages::STORAGE,
        });
        let dummy_lut = lut_texture(&gpu, &[Color::rgb(0.0, 0.0, 0.0), Color::rgb(1.0, 1.0, 1.0)]);
        Resources {
            gpu,
            pipes,
            cache: HashMap::new(),
            luts: HashMap::new(),
            dummy,
            dummy_lut,
            frame: 0,
            stats: RenderStats::default(),
            glyphs: None,
        }
    }

    /// Drops cached buffers and LUTs not used for a while.
    pub fn evict(&mut self) {
        let frame = self.frame;
        self.cache.retain(|_, c| frame - c.frame < 120);
        self.luts.retain(|_, (_, _, f)| frame - *f < 120);
    }
}

/// What a pipeline's `prepare` sees for one frame.
pub(crate) struct Frame<'a> {
    pub res: &'a mut Resources,
    pub pipes: Arc<Pipelines>,
    pub uniforms: &'a mut Vec<u8>,
    pub ubuf: wgpu::Buffer,
    /// Device pixels per figure unit.
    pub ppu: f64,
    /// Target size in device pixels.
    pub size: [u32; 2],
}

impl Frame<'_> {
    pub fn device(&self) -> &wgpu::Device {
        &self.res.gpu.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.res.gpu.queue
    }

    /// Appends a uniform block; returns its dynamic offset.
    pub fn push_uniform<T: Pod>(&mut self, u: &T) -> u32 {
        let bytes = bytemuck::bytes_of(u);
        assert!(bytes.len() <= UNIFORM_ALIGN, "uniform block larger than {UNIFORM_ALIGN} bytes");
        let off = self.uniforms.len();
        self.uniforms.extend_from_slice(bytes);
        self.uniforms.resize(off + UNIFORM_ALIGN, 0);
        off as u32
    }

    /// Binding for a uniform block of type `T` in the frame's uniform ring (use with a dynamic offset).
    pub fn uniform_binding<T: Pod>(&self) -> wgpu::BindingResource<'_> {
        wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: &self.ubuf,
            offset: 0,
            size: wgpu::BufferSize::new(std::mem::size_of::<T>() as u64),
        })
    }

    /// A 16-byte placeholder for unused storage bindings.
    pub fn dummy(&self) -> wgpu::Buffer {
        self.res.dummy.clone()
    }

    pub fn dummy_lut(&self) -> wgpu::TextureView {
        self.res.dummy_lut.clone()
    }

    /// A storage buffer for `b` (cached by its key; a 16-byte dummy when empty).
    pub fn storage<T: Pod>(&mut self, b: &Buf<T>) -> wgpu::Buffer {
        if b.data.is_empty() {
            return self.dummy();
        }
        self.buffer(b, wgpu::BufferUsages::STORAGE)
    }

    /// A buffer with `usage` for `b`, reusing the cached upload when its key is unchanged.
    pub fn buffer<T: Pod>(&mut self, b: &Buf<T>, usage: wgpu::BufferUsages) -> wgpu::Buffer {
        let bytes: &[u8] = bytemuck::cast_slice(b.data.as_slice());
        match b.key {
            None => self.transient(bytes, usage),
            Some(k) => self.cached((k.uid, k.part), k.rev, bytes, usage),
        }
    }

    /// A buffer uploaded this frame only.
    pub fn transient(&mut self, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.res.stats.transient_bytes += bytes.len() as u64;
        self.res.gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytes,
            usage,
        })
    }

    /// A buffer cached across frames under `key`, re-uploaded only when `rev` changes.
    pub fn cached(&mut self, key: (u64, u8), rev: u64, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        let frame = self.res.frame;
        if let Some(c) = self.res.cache.get_mut(&key) {
            if c.rev == rev && c.buf.usage().contains(usage) {
                c.frame = frame;
                return c.buf.clone();
            }
            if c.buf.size() >= bytes.len() as u64 && c.buf.usage().contains(usage) {
                self.res.stats.data_bytes += bytes.len() as u64;
                self.res.gpu.queue.write_buffer(&c.buf, 0, bytes);
                c.rev = rev;
                c.frame = frame;
                return c.buf.clone();
            }
        }
        let size = (bytes.len() as u64 * 3 / 2).max(16).next_multiple_of(4);
        let buf = self.res.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.res.stats.data_bytes += bytes.len() as u64;
        self.res.gpu.queue.write_buffer(&buf, 0, bytes);
        self.res.cache.insert(key, Cached { rev, buf: buf.clone(), frame });
        buf
    }

    /// The 1D texture for a colormap LUT (cached by `Arc` identity).
    pub fn lut(&mut self, lut: &Arc<Vec<Color>>) -> wgpu::TextureView {
        let key = Arc::as_ptr(lut) as usize;
        let frame = self.res.frame;
        if let Some((_, v, f)) = self.res.luts.get_mut(&key) {
            *f = frame;
            return v.clone();
        }
        let v = lut_texture(&self.res.gpu, lut);
        self.res.luts.insert(key, (lut.clone(), v.clone(), frame));
        v
    }
}

/// A `colors.len()` × 1 RGBA8 texture.
pub(crate) fn lut_texture(gpu: &Gpu, colors: &[Color]) -> wgpu::TextureView {
    let data: Vec<u8> = colors.iter().flat_map(|c| c.to_rgba8()).collect();
    let tex = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("colormap"),
            size: wgpu::Extent3d { width: colors.len() as u32, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

pub(crate) fn premul(c: Color) -> [f32; 4] {
    [c.r * c.a, c.g * c.a, c.b * c.a, c.a]
}

pub(crate) fn straight(c: Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

/// WGSL `CMap` (see common.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub(crate) struct CMapU {
    pub range: [f32; 2],
    pub n: f32,
    pub alpha: f32,
    pub lowclip: [f32; 4],
    pub highclip: [f32; 4],
    pub nan_color: [f32; 4],
}

impl From<&ColorMapping> for CMapU {
    fn from(m: &ColorMapping) -> CMapU {
        let first = m.lut.first().copied().unwrap_or(Color::TRANSPARENT);
        let last = m.lut.last().copied().unwrap_or(Color::TRANSPARENT);
        let lo = m.lowclip.unwrap_or(first);
        let hi = m.highclip.unwrap_or(last);
        CMapU {
            range: m.range,
            n: m.lut.len().max(1) as f32,
            alpha: m.alpha,
            lowclip: straight(lo.with_alpha(lo.a * m.alpha)),
            highclip: straight(hi.with_alpha(hi.a * m.alpha)),
            nan_color: straight(m.nan_color),
        }
    }
}
