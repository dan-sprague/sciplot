//! Per-context renderer: owns GPU caches for one window or one export, turns a `DrawList` into
//! a single MSAA render pass.

use super::pipelines::Pipelines;
use super::{Gpu, MSAA, TARGET_FORMAT};
use crate::color::Color;
use crate::error::{Error, Result};
use crate::scene::SceneCache;
use crate::scene::drawlist::{
    Buf, ColorMapping, DrawList, MarkersPrim, MeshPrim, MeshVertex, Prim, PrimColor, Rect,
    RectPrim, Space,
};
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlobalsU {
    target_px: [f32; 2],
    ppu: f32,
    _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub(crate) struct CMapU {
    range: [f32; 2],
    n: f32,
    alpha: f32,
    lowclip: [f32; 4],
    highclip: [f32; 4],
    nan_color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MeshU {
    xform: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SpriteU {
    xform: [f32; 4],
    color: [f32; 4],
    stroke_color: [f32; 4],
    size: f32,
    stroke: f32,
    shape: u32,
    col_mode: u32,
    size_stride: u32,
    rotation: f32,
    _p0: u32,
    _p1: u32,
    cm: CMapU,
}

fn premul(c: Color) -> [f32; 4] {
    [c.r * c.a, c.g * c.a, c.b * c.a, c.a]
}

fn straight(c: Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

pub(crate) fn cmap_u(m: &ColorMapping) -> CMapU {
    let first = m.lut.first().copied().unwrap_or(Color::TRANSPARENT);
    let last = m.lut.last().copied().unwrap_or(Color::TRANSPARENT);
    CMapU {
        range: m.range,
        n: m.lut.len().max(1) as f32,
        alpha: m.alpha,
        lowclip: straight(
            m.lowclip
                .unwrap_or(first)
                .with_alpha(m.lowclip.unwrap_or(first).a * m.alpha),
        ),
        highclip: straight(
            m.highclip
                .unwrap_or(last)
                .with_alpha(m.highclip.unwrap_or(last).a * m.alpha),
        ),
        nan_color: straight(m.nan_color),
    }
}

struct Cached {
    rev: u64,
    buf: wgpu::Buffer,
    frame: u64,
}

enum Cmd {
    Mesh {
        vb: wgpu::Buffer,
        count: u32,
        offset: u32,
    },
    Sprite {
        bg: wgpu::BindGroup,
        instances: u32,
        offset: u32,
    },
}

struct Draw {
    scissor: [u32; 4],
    cmd: Cmd,
}

const UNIFORM_ALIGN: usize = 256;

/// Per-frame upload counters (for perf tests).
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    /// Bytes written to cached (keyed) buffers: plot data re-uploads.
    pub data_bytes: u64,
    /// Bytes written to per-frame buffers (decorations, text).
    pub transient_bytes: u64,
    pub draws: u32,
}

pub(crate) struct Renderer {
    gpu: Arc<Gpu>,
    pipes: Arc<Pipelines>,
    msaa: Option<(wgpu::TextureView, [u32; 2])>,
    cache: HashMap<(u64, u8), Cached>,
    luts: HashMap<usize, (Arc<Vec<Color>>, wgpu::TextureView, u64)>,
    frame: u64,
    uniforms: Option<(wgpu::Buffer, usize)>,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    dummy: wgpu::Buffer,
    dummy_lut: wgpu::TextureView,
    /// Scene-building memo for this context.
    pub scene: SceneCache,
    warned: bool,
    /// Counters for the last rendered frame.
    pub stats: RenderStats,
}

impl Renderer {
    pub fn new(gpu: Arc<Gpu>) -> Renderer {
        let pipes = gpu.pipelines();
        let device = &gpu.device;
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<GlobalsU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &pipes.globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&pipes.sampler),
                },
            ],
        });
        let dummy = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("dummy"),
            contents: &[0u8; 16],
            usage: wgpu::BufferUsages::STORAGE,
        });
        let dummy_lut = lut_texture(
            &gpu,
            &[Color::rgb(0.0, 0.0, 0.0), Color::rgb(1.0, 1.0, 1.0)],
        );
        Renderer {
            gpu,
            pipes,
            msaa: None,
            cache: HashMap::new(),
            luts: HashMap::new(),
            frame: 0,
            uniforms: None,
            globals_buf,
            globals_bg,
            dummy,
            dummy_lut,
            scene: SceneCache::new(),
            warned: false,
            stats: RenderStats::default(),
        }
    }

    pub fn gpu(&self) -> &Arc<Gpu> {
        &self.gpu
    }

    fn msaa_view(&mut self, size: [u32; 2]) -> wgpu::TextureView {
        if let Some((v, s)) = &self.msaa {
            if *s == size {
                return v.clone();
            }
        }
        let tex = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: MSAA,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let v = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.msaa = Some((v.clone(), size));
        v
    }

    /// A storage buffer for `b`, reusing the cached upload when its key is unchanged.
    fn storage<T: Pod>(&mut self, b: &Buf<T>) -> wgpu::Buffer {
        if b.data.is_empty() {
            return self.dummy.clone();
        }
        let bytes: &[u8] = bytemuck::cast_slice(b.data.as_slice());
        self.upload(
            b.key.map(|k| ((k.uid, k.part), k.rev)),
            bytes,
            wgpu::BufferUsages::STORAGE,
        )
    }

    fn upload(
        &mut self,
        key: Option<((u64, u8), u64)>,
        bytes: &[u8],
        usage: wgpu::BufferUsages,
    ) -> wgpu::Buffer {
        let device = &self.gpu.device;
        match key {
            None => {
                self.stats.transient_bytes += bytes.len() as u64;
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytes,
                    usage,
                })
            }
            Some((k, rev)) => {
                if let Some(c) = self.cache.get_mut(&k) {
                    if c.rev == rev {
                        c.frame = self.frame;
                        return c.buf.clone();
                    }
                    if c.buf.size() >= bytes.len() as u64 && c.buf.usage().contains(usage) {
                        self.stats.data_bytes += bytes.len() as u64;
                        self.gpu.queue.write_buffer(&c.buf, 0, bytes);
                        c.rev = rev;
                        c.frame = self.frame;
                        return c.buf.clone();
                    }
                }
                let size = (bytes.len() as u64 * 3 / 2).max(16).next_multiple_of(4);
                let buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size,
                    usage: usage | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.stats.data_bytes += bytes.len() as u64;
                self.gpu.queue.write_buffer(&buf, 0, bytes);
                self.cache.insert(
                    k,
                    Cached {
                        rev,
                        buf: buf.clone(),
                        frame: self.frame,
                    },
                );
                buf
            }
        }
    }

    fn lut(&mut self, lut: &Arc<Vec<Color>>) -> wgpu::TextureView {
        let key = Arc::as_ptr(lut) as usize;
        if let Some((_, v, f)) = self.luts.get_mut(&key) {
            *f = self.frame;
            return v.clone();
        }
        let v = lut_texture(&self.gpu, lut);
        self.luts.insert(key, (lut.clone(), v.clone(), self.frame));
        v
    }

    /// Encodes one frame of `dl` into `target` (size in device pixels).
    pub fn render(
        &mut self,
        dl: &DrawList,
        target: &wgpu::TextureView,
        size: [u32; 2],
        ppu: f64,
    ) -> wgpu::CommandBuffer {
        self.frame += 1;
        self.stats = RenderStats::default();
        let msaa = self.msaa_view(size);
        let g = GlobalsU {
            target_px: [size[0] as f32, size[1] as f32],
            ppu: ppu as f32,
            _pad: 0.0,
        };
        self.gpu
            .queue
            .write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&g));

        let mut uniforms: Vec<u8> = Vec::new();
        let mut push_u = |bytes: &[u8]| -> u32 {
            let off = uniforms.len();
            uniforms.extend_from_slice(bytes);
            uniforms.resize(off + bytes.len().next_multiple_of(UNIFORM_ALIGN), 0);
            off as u32
        };

        // Pass 1: prepare buffers and uniforms (bind groups need the uniform buffer, so they are
        // created in pass 2).
        enum Pending {
            Mesh {
                vb: wgpu::Buffer,
                count: u32,
                off: u32,
            },
            Sprite {
                m: MarkersPrim,
                off: u32,
                pos: wgpu::Buffer,
                col: wgpu::Buffer,
                val: wgpu::Buffer,
                sz: wgpu::Buffer,
                lut: wgpu::TextureView,
            },
        }
        let mut pending: Vec<([u32; 4], Pending)> = Vec::new();
        let full = [0, 0, size[0], size[1]];
        for item in &dl.items {
            let scissor = match item.clip {
                Some(r) => match scissor(r, ppu, size) {
                    Some(s) => s,
                    None => continue,
                },
                None => full,
            };
            let xform: [f32; 4] = match item.space {
                Space::Figure => [ppu as f32, ppu as f32, 0.0, 0.0],
                Space::Data(i) => dl.axes[i as usize].affine(ppu).map(|v| v as f32),
            };
            match &item.prim {
                Prim::Rects(rects) => {
                    let verts = rect_vertices(rects, ppu);
                    if verts.is_empty() {
                        continue;
                    }
                    let vb = self.upload(
                        None,
                        bytemuck::cast_slice(&verts),
                        wgpu::BufferUsages::VERTEX,
                    );
                    let off = push_u(bytemuck::bytes_of(&MeshU {
                        xform: [1.0, 1.0, 0.0, 0.0],
                    }));
                    pending.push((
                        scissor,
                        Pending::Mesh {
                            vb,
                            count: verts.len() as u32,
                            off,
                        },
                    ));
                }
                Prim::Mesh(MeshPrim { verts }) => {
                    if verts.data.is_empty() {
                        continue;
                    }
                    let vb = self.upload(
                        verts.key.map(|k| ((k.uid, k.part), k.rev)),
                        bytemuck::cast_slice(verts.data.as_slice()),
                        wgpu::BufferUsages::VERTEX,
                    );
                    let off = push_u(bytemuck::bytes_of(&MeshU { xform }));
                    pending.push((
                        scissor,
                        Pending::Mesh {
                            vb,
                            count: verts.data.len() as u32,
                            off,
                        },
                    ));
                }
                Prim::Markers(m) => {
                    if m.pos.data.is_empty() {
                        continue;
                    }
                    let pos = self.storage(&m.pos);
                    let (col_mode, color, col, val, cm, lut) = match &m.color {
                        PrimColor::Uniform(c) => (
                            0,
                            premul(*c),
                            self.dummy.clone(),
                            self.dummy.clone(),
                            CMapU::default(),
                            self.dummy_lut.clone(),
                        ),
                        PrimColor::PerElement(b) => (
                            1,
                            [0.0; 4],
                            self.storage(b),
                            self.dummy.clone(),
                            CMapU::default(),
                            self.dummy_lut.clone(),
                        ),
                        PrimColor::Values(b, map) => (
                            2,
                            [0.0; 4],
                            self.dummy.clone(),
                            self.storage(b),
                            cmap_u(map),
                            self.lut(&map.lut),
                        ),
                    };
                    let (size_stride, sz) = match &m.sizes {
                        Some(s) => (1, self.storage(s)),
                        None => (0, self.dummy.clone()),
                    };
                    let u = SpriteU {
                        xform,
                        color,
                        stroke_color: premul(m.stroke_color),
                        size: m.size,
                        stroke: m.stroke_width,
                        shape: m.marker.shader_id(),
                        col_mode,
                        size_stride,
                        rotation: m.rotation,
                        _p0: 0,
                        _p1: 0,
                        cm,
                    };
                    let off = push_u(bytemuck::bytes_of(&u));
                    pending.push((
                        scissor,
                        Pending::Sprite {
                            m: m.clone(),
                            off,
                            pos,
                            col,
                            val,
                            sz,
                            lut,
                        },
                    ));
                }
                Prim::Lines(_) | Prim::Glyphs(_) | Prim::Field(_) => {
                    if !self.warned {
                        log::debug!("ezviz: primitive not yet supported by the GPU backend");
                        self.warned = true;
                    }
                }
            }
        }

        // Uniform ring.
        if uniforms.is_empty() {
            uniforms.resize(UNIFORM_ALIGN, 0);
        }
        let need = uniforms.len();
        let ubuf = match &self.uniforms {
            Some((b, cap)) if *cap >= need => b.clone(),
            _ => {
                let cap = need.next_power_of_two().max(64 * 1024);
                let b = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("uniforms"),
                    size: cap as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.uniforms = Some((b.clone(), cap));
                b
            }
        };
        self.gpu.queue.write_buffer(&ubuf, 0, &uniforms);

        // Pass 2: bind groups.
        let device = &self.gpu.device;
        let ubind = |size: usize| {
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &ubuf,
                offset: 0,
                size: wgpu::BufferSize::new(size as u64),
            })
        };
        let mesh_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh"),
            layout: &self.pipes.mesh_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ubind(std::mem::size_of::<MeshU>()),
            }],
        });
        self.stats.draws = pending.len() as u32;
        let draws: Vec<Draw> = pending
            .into_iter()
            .map(|(scissor, p)| match p {
                Pending::Mesh { vb, count, off } => Draw {
                    scissor,
                    cmd: Cmd::Mesh {
                        vb,
                        count,
                        offset: off,
                    },
                },
                Pending::Sprite {
                    m,
                    off,
                    pos,
                    col,
                    val,
                    sz,
                    lut,
                } => {
                    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("sprite"),
                        layout: &self.pipes.sprite_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: ubind(std::mem::size_of::<SpriteU>()),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: pos.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: col.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 3,
                                resource: val.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 4,
                                resource: sz.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 5,
                                resource: wgpu::BindingResource::TextureView(&lut),
                            },
                        ],
                    });
                    Draw {
                        scissor,
                        cmd: Cmd::Sprite {
                            bg,
                            instances: m.pos.data.len() as u32,
                            offset: off,
                        },
                    }
                }
            })
            .collect();

        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });
        {
            let bg = dl.background;
            let clear = wgpu::Color {
                r: (bg.r * bg.a) as f64,
                g: (bg.g * bg.a) as f64,
                b: (bg.b * bg.a) as f64,
                a: bg.a as f64,
            };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("figure"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &msaa,
                    depth_slice: None,
                    resolve_target: Some(target),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bg, &[]);
            for d in &draws {
                let [x, y, w, h] = d.scissor;
                pass.set_scissor_rect(x, y, w, h);
                match &d.cmd {
                    Cmd::Mesh { vb, count, offset } => {
                        pass.set_pipeline(&self.pipes.mesh);
                        pass.set_bind_group(1, &mesh_bg, &[*offset]);
                        pass.set_vertex_buffer(0, vb.slice(..));
                        pass.draw(0..*count, 0..1);
                    }
                    Cmd::Sprite {
                        bg,
                        instances,
                        offset,
                    } => {
                        pass.set_pipeline(&self.pipes.sprite);
                        pass.set_bind_group(1, bg, &[*offset]);
                        pass.draw(0..4, 0..*instances);
                    }
                }
            }
        }

        // Evict cache entries unused for a while.
        let frame = self.frame;
        self.cache.retain(|_, c| frame - c.frame < 120);
        self.luts.retain(|_, (_, _, f)| frame - *f < 120);
        enc.finish()
    }

    /// Renders `dl` offscreen at `ppu` and reads back straight-alpha RGBA8 rows.
    pub fn render_rgba(&mut self, dl: &DrawList, ppu: f64) -> Result<(u32, u32, Vec<u8>)> {
        let w = (dl.size[0] * ppu).round().max(1.0) as u32;
        let h = (dl.size[1] * ppu).round().max(1.0) as u32;
        let max = self.gpu.max_texture_size();
        if w > max || h > max {
            return Err(Error::TooLarge(format!(
                "a {w}x{h} px image exceeds the GPU texture limit of {max} px; lower px_per_unit/dpi or the figure size"
            )));
        }
        let device = self.gpu.device.clone();
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let frame_cmd = self.render(dl, &view, [w, h], ppu);

        let unpadded = w * 4;
        let padded = unpadded.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback"),
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        let idx = self.gpu.queue.submit([frame_cmd, enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(idx),
                timeout: None,
            })
            .map_err(|e| Error::Gpu(e.to_string()))?;
        rx.recv()
            .map_err(|e| Error::Gpu(e.to_string()))?
            .map_err(|e| Error::Gpu(e.to_string()))?;
        let mut rgba = Vec::with_capacity((unpadded * h) as usize);
        {
            let data = slice
                .get_mapped_range()
                .map_err(|e| Error::Gpu(e.to_string()))?;
            for row in data.chunks_exact(padded as usize) {
                for px in row[..unpadded as usize].chunks_exact(4) {
                    // BGRA premultiplied -> RGBA straight.
                    let a = px[3];
                    let un = |v: u8| {
                        if a == 0 || a == 255 {
                            v
                        } else {
                            ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8
                        }
                    };
                    rgba.extend_from_slice(&[un(px[2]), un(px[1]), un(px[0]), a]);
                }
            }
        }
        buf.unmap();
        Ok((w, h, rgba))
    }
}

fn lut_texture(gpu: &Gpu, colors: &[Color]) -> wgpu::TextureView {
    let data: Vec<u8> = colors.iter().flat_map(|c| c.to_rgba8()).collect();
    let tex = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("colormap"),
            size: wgpu::Extent3d {
                width: colors.len() as u32,
                height: 1,
                depth_or_array_layers: 1,
            },
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

/// Clip rectangle (units) -> scissor (device px), clamped; `None` if empty.
fn scissor(r: Rect, ppu: f64, size: [u32; 2]) -> Option<[u32; 4]> {
    let x0 = (r.x * ppu).round().clamp(0.0, size[0] as f64) as u32;
    let y0 = (r.y * ppu).round().clamp(0.0, size[1] as f64) as u32;
    let x1 = (r.right() * ppu).round().clamp(0.0, size[0] as f64) as u32;
    let y1 = (r.bottom() * ppu).round().clamp(0.0, size[1] as f64) as u32;
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
}

/// Snaps the center of a thin line so it covers whole device pixels: odd widths center on a pixel
/// center, even widths on a pixel edge. The width itself is never changed.
fn snap_center(c: f64, width_px: f64) -> f64 {
    if (width_px.round() as i64) % 2 == 1 {
        c.floor() + 0.5
    } else {
        c.round()
    }
}

/// Rect primitives -> triangle vertices in device pixels.
fn rect_vertices(rects: &[RectPrim], ppu: f64) -> Vec<MeshVertex> {
    let mut v = Vec::with_capacity(rects.len() * 6);
    for r in rects {
        let (mut x0, mut y0) = (r.rect.x * ppu, r.rect.y * ppu);
        let (w, h) = (r.rect.w * ppu, r.rect.h * ppu);
        if r.snap {
            if w <= h {
                x0 = snap_center(x0 + 0.5 * w, w) - 0.5 * w;
            }
            if h <= w {
                y0 = snap_center(y0 + 0.5 * h, h) - 0.5 * h;
            }
        }
        let (x1, y1) = (x0 + w, y0 + h);
        let c = r.color.to_premul_u32();
        let p = |x: f64, y: f64| MeshVertex {
            pos: [x as f32, y as f32],
            color: c,
        };
        v.extend_from_slice(&[
            p(x0, y0),
            p(x1, y0),
            p(x0, y1),
            p(x1, y0),
            p(x1, y1),
            p(x0, y1),
        ]);
    }
    v
}
