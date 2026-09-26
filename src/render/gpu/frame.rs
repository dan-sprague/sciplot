//! Per-frame helpers shared by all pipelines: cached/transient vertex buffers and data textures,
//! the uniform ring, and the draw command type.

use super::Gpu;
use super::pipelines::{Layouts, Pipelines};
use crate::color::Color;
use crate::data::points::split_append_rev;
use crate::scene::drawlist::{Buf, ColorMapping};
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Uniform blocks are placed at multiples of this (the WebGPU minimum dynamic-offset alignment;
/// raised to the device's requirement if larger).
pub(crate) const UNIFORM_ALIGN: usize = 256;

/// Cache tags: one plot buffer (`BufKey`) can have several GPU representations.
pub(crate) mod tag {
    /// The data as-is (colors, values, sizes, mesh vertices).
    pub const RAW: u8 = 0;
    /// Points with one spare element before and after (see `Frame::points`).
    pub const POINTS: u8 = 1;
    /// Dash arc lengths of a polyline.
    pub const DASH: u8 = 2;
    /// Line neighbours past exact duplicate points.
    pub const PREV: u8 = 3;
    pub const NEXT: u8 = 4;
}

/// Byte offset of element 0 in a buffer from [`Frame::points`].
pub(crate) const POINTS_OFFSET: u64 = 8;

/// One draw call.
pub(crate) struct DrawCmd {
    pub pipeline: wgpu::RenderPipeline,
    /// Bind group 1 (group 0 is the per-frame globals).
    pub bind: wgpu::BindGroup,
    /// Dynamic offset of this draw's uniform block.
    pub offset: u32,
    /// Vertex buffers by slot, each with the byte offset it is bound at.
    pub vbs: Vec<(wgpu::Buffer, u64)>,
    pub vertices: Range<u32>,
    pub instances: Range<u32>,
}

/// Per-frame upload counters (for perf tests).
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    /// Bytes written to cached (keyed) buffers and textures: plot data re-uploads.
    pub data_bytes: u64,
    /// Bytes written to per-frame buffers (decorations, text).
    pub transient_bytes: u64,
    pub draws: u32,
}

pub(crate) struct Cached {
    pub rev: u64,
    pub buf: wgpu::Buffer,
    pub frame: u64,
    /// Points buffers: the revision whose closed-loop spare elements are written.
    pub pads: Option<u64>,
    /// Points buffers: `(revision, whether consecutive points repeat exactly)`.
    pub dups: Option<(u64, bool)>,
}

pub(crate) struct CachedTex {
    pub rev: u64,
    pub tex: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub frame: u64,
}

/// GPU resources that persist across frames of one render context.
pub(crate) struct Resources {
    pub gpu: Arc<Gpu>,
    pub layouts: Arc<Layouts>,
    pub cache: HashMap<(u64, u8, u8), Cached>,
    pub textures: HashMap<(u64, u8, u32), CachedTex>,
    pub luts: HashMap<usize, (Arc<Vec<Color>>, wgpu::TextureView, u64)>,
    pub dummy_lut: wgpu::TextureView,
    /// A 1×1 `R32Float` texture for unused data-texture bindings.
    pub dummy_tex: wgpu::TextureView,
    pub frame: u64,
    pub stats: RenderStats,
    /// The glyph atlas (created on first use).
    pub glyphs: Option<super::pipelines::glyph::GlyphCache>,
}

impl Resources {
    pub fn new(gpu: Arc<Gpu>) -> Resources {
        let layouts = gpu.layouts();
        let dummy_lut = lut_texture(&gpu, &[Color::rgb(0.0, 0.0, 0.0), Color::rgb(1.0, 1.0, 1.0)]);
        let dummy_tex = r32_texture(&gpu.device, 1, 1).create_view(&wgpu::TextureViewDescriptor::default());
        Resources {
            gpu,
            layouts,
            cache: HashMap::new(),
            textures: HashMap::new(),
            luts: HashMap::new(),
            dummy_lut,
            dummy_tex,
            frame: 0,
            stats: RenderStats::default(),
            glyphs: None,
        }
    }

    /// Drops cached buffers, textures and LUTs not used for a while.
    pub fn evict(&mut self) {
        let frame = self.frame;
        self.cache.retain(|_, c| frame - c.frame < 120);
        self.textures.retain(|_, c| frame - c.frame < 120);
        self.luts.retain(|_, (_, _, f)| frame - *f < 120);
    }
}

/// What a pipeline's `prepare` sees for one frame.
pub(crate) struct Frame<'a> {
    pub res: &'a mut Resources,
    /// The pipelines for this frame's target format.
    pub pipes: Arc<Pipelines>,
    pub uniforms: &'a mut Vec<u8>,
    pub ubuf: wgpu::Buffer,
    /// Uniform block alignment in bytes.
    pub ualign: usize,
    /// Device pixels per figure unit.
    pub ppu: f64,
    /// Target size in device pixels.
    pub size: [u32; 2],
}

impl Frame<'_> {
    pub fn device(&self) -> &wgpu::Device {
        &self.res.gpu.device
    }

    pub fn layouts(&self) -> &Layouts {
        &self.res.layouts
    }

    /// Appends a uniform block; returns its dynamic offset.
    pub fn push_uniform<T: Pod>(&mut self, u: &T) -> u32 {
        let bytes = bytemuck::bytes_of(u);
        assert!(bytes.len() <= UNIFORM_ALIGN, "uniform block larger than {UNIFORM_ALIGN} bytes");
        let off = self.uniforms.len();
        self.uniforms.extend_from_slice(bytes);
        self.uniforms.resize(off + self.ualign, 0);
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

    pub fn dummy_lut(&self) -> wgpu::TextureView {
        self.res.dummy_lut.clone()
    }

    pub fn dummy_tex(&self) -> wgpu::TextureView {
        self.res.dummy_tex.clone()
    }

    /// A vertex buffer holding `b` as-is (cached by its key; at least 16 bytes).
    pub fn vertex<T: Pod>(&mut self, b: &Buf<T>) -> wgpu::Buffer {
        let bytes: &[u8] = bytemuck::cast_slice(b.data.as_slice());
        match b.key {
            None => self.transient(bytes, wgpu::BufferUsages::VERTEX),
            Some(k) => self.cached((k.uid, k.part, tag::RAW), k.rev, bytes, wgpu::BufferUsages::VERTEX),
        }
    }

    /// A vertex buffer holding the points of `b` with one spare element before and after them
    /// (element `k` at byte `8 * (k + 1)`), so line segments can bind it at four offsets to read
    /// `p[i - 1] .. p[i + 2]`. For `closed` loops the spare elements hold `p[n - 2]` and `p[1]`
    /// (the neighbours across the seam); otherwise their content is unspecified.
    ///
    /// Append-only buffers (`append`) whose GPU copy is an older revision of the same
    /// generation only upload the new tail.
    pub fn points(&mut self, b: &Buf<[f32; 2]>, append: bool, closed: bool) -> wgpu::Buffer {
        let p = b.data.as_slice();
        let n = p.len();
        let pads = (closed && n >= 2).then(|| [p[n - 2], p[1]]);
        let Some(k) = b.key else {
            let mut v = Vec::with_capacity(n + 2);
            v.push(pads.map_or([0.0; 2], |q| q[0]));
            v.extend_from_slice(p);
            v.push(pads.map_or([0.0; 2], |q| q[1]));
            return self.transient(bytemuck::cast_slice(&v), wgpu::BufferUsages::VERTEX);
        };
        let key = (k.uid, k.part, tag::POINTS);
        let bytes: &[u8] = bytemuck::cast_slice(p);
        let need = (n as u64 + 2) * 8;
        let frame = self.res.frame;
        let gpu = self.res.gpu.clone();
        let stats = &mut self.res.stats;
        let entry = match self.res.cache.get_mut(&key) {
            Some(c) if c.rev == k.rev => Some(c),
            Some(c) if c.buf.size() >= need => {
                let (g0, n0) = split_append_rev(c.rev);
                let (g1, n1) = split_append_rev(k.rev);
                let start = if append && g0 == g1 && n0 < n1 && n1 == n { n0 } else { 0 };
                gpu.queue.write_buffer(&c.buf, POINTS_OFFSET + start as u64 * 8, &bytes[start * 8..]);
                stats.data_bytes += (bytes.len() - start * 8) as u64;
                c.rev = k.rev;
                c.pads = None;
                Some(c)
            }
            _ => None,
        };
        let c = match entry {
            Some(c) => c,
            None => {
                let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("points"),
                    size: (need * 3 / 2).next_multiple_of(8),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                gpu.queue.write_buffer(&buf, POINTS_OFFSET, bytes);
                stats.data_bytes += bytes.len() as u64;
                self.res
                    .cache
                    .entry(key)
                    .insert_entry(Cached { rev: k.rev, buf, frame, pads: None, dups: None })
                    .into_mut()
            }
        };
        c.frame = frame;
        if let Some([a, z]) = pads
            && c.pads != Some(k.rev)
        {
            gpu.queue.write_buffer(&c.buf, 0, bytemuck::bytes_of(&a));
            gpu.queue.write_buffer(&c.buf, (n as u64 + 1) * 8, bytemuck::bytes_of(&z));
            stats.data_bytes += 16;
            c.pads = Some(k.rev);
        }
        c.buf.clone()
    }

    /// Whether two consecutive points of `b` are exactly equal (both finite). Remembered per
    /// revision next to the buffer from [`Frame::points`] (call that first); append-only
    /// revisions only scan the new tail.
    pub fn has_duplicates(&mut self, b: &Buf<[f32; 2]>) -> bool {
        let p = b.data.as_slice();
        let scan = |p: &[[f32; 2]]| p.windows(2).any(|w| w[0] == w[1] && w[0].iter().all(|v| v.is_finite()));
        let Some(k) = b.key else { return scan(p) };
        let Some(c) = self.res.cache.get_mut(&(k.uid, k.part, tag::POINTS)) else { return scan(p) };
        let has = match c.dups {
            Some((rev, has)) if rev == k.rev => return has,
            Some((rev, has)) => {
                let (g0, n0) = split_append_rev(rev);
                let (g1, n1) = split_append_rev(k.rev);
                if g0 == g1 && n0 < n1 && n1 == p.len() { has || scan(&p[n0.saturating_sub(1)..]) } else { scan(p) }
            }
            None => scan(p),
        };
        c.dups = Some((k.rev, has));
        has
    }

    /// A buffer uploaded this frame only.
    pub fn transient(&mut self, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.res.stats.transient_bytes += bytes.len() as u64;
        let mut padded;
        let contents = if bytes.len() < 16 {
            padded = bytes.to_vec();
            padded.resize(16, 0);
            &padded[..]
        } else {
            bytes
        };
        self.res.gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents, usage })
    }

    /// Like [`Frame::cached`], computing the bytes only when they have to be uploaded.
    pub fn cached_with(
        &mut self,
        key: (u64, u8, u8),
        rev: u64,
        usage: wgpu::BufferUsages,
        bytes: impl FnOnce() -> Vec<u8>,
    ) -> wgpu::Buffer {
        let frame = self.res.frame;
        if let Some(c) = self.res.cache.get_mut(&key)
            && c.rev == rev
            && c.buf.usage().contains(usage)
        {
            c.frame = frame;
            return c.buf.clone();
        }
        self.cached(key, rev, &bytes(), usage)
    }

    /// A buffer cached across frames under `key`, re-uploaded only when `rev` changes.
    pub fn cached(&mut self, key: (u64, u8, u8), rev: u64, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
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
        self.res.cache.insert(key, Cached { rev, buf: buf.clone(), frame, pads: None, dups: None });
        buf
    }

    /// An `R32Float` texture holding the `w × h` block at column `x0`, row `y0` of the row-major
    /// array `data` (rows of `stride` values). Cached under `key` (re-uploaded when `rev`
    /// changes); `None` uploads for this frame only.
    #[allow(clippy::too_many_arguments)]
    pub fn data_texture(
        &mut self,
        key: Option<(u64, u8, u32)>,
        rev: u64,
        data: &[f32],
        stride: usize,
        x0: usize,
        y0: usize,
        w: usize,
        h: usize,
    ) -> wgpu::TextureView {
        let frame = self.res.frame;
        let (w32, h32) = (w as u32, h as u32);
        let upload = |gpu: &Gpu, tex: &wgpu::Texture| {
            gpu.queue.write_texture(
                tex.as_image_copy(),
                bytemuck::cast_slice(data),
                wgpu::TexelCopyBufferLayout {
                    offset: ((y0 * stride + x0) * 4) as u64,
                    bytes_per_row: Some(stride as u32 * 4),
                    rows_per_image: Some(h32),
                },
                wgpu::Extent3d { width: w32, height: h32, depth_or_array_layers: 1 },
            );
        };
        let bytes = (w * h * 4) as u64;
        if let Some(key) = key
            && let Some(c) = self.res.textures.get_mut(&key)
            && (c.tex.width(), c.tex.height()) == (w32, h32)
        {
            if c.rev != rev {
                upload(&self.res.gpu, &c.tex);
                self.res.stats.data_bytes += bytes;
                c.rev = rev;
            }
            c.frame = frame;
            return c.view.clone();
        }
        let tex = r32_texture(&self.res.gpu.device, w32, h32);
        upload(&self.res.gpu, &tex);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        match key {
            Some(key) => {
                self.res.stats.data_bytes += bytes;
                self.res.textures.insert(key, CachedTex { rev, tex, view: view.clone(), frame });
            }
            None => self.res.stats.transient_bytes += bytes,
        }
        view
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

fn r32_texture(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("data"),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
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
