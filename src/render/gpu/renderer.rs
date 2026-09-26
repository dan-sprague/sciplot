//! Per-context renderer: owns GPU caches for one window or one export and turns a `DrawList`
//! into a single 4x MSAA render pass (painter's order, scissor per item).

use super::frame::{DrawCmd, Frame, RenderStats, Resources, UNIFORM_ALIGN};
use super::pipelines;
use super::{Gpu, MSAA, TARGET_FORMAT};
use crate::error::{Error, Result};
use crate::scene::SceneCache;
use crate::scene::drawlist::{DrawList, Prim, Rect, Space};
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlobalsU {
    target_px: [f32; 2],
    ppu: f32,
    _pad: f32,
}

pub(crate) struct Renderer {
    res: Resources,
    msaa: Option<(wgpu::TextureView, [u32; 2])>,
    uniforms: Option<(wgpu::Buffer, usize)>,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    /// Scene-building memo for this context.
    pub scene: SceneCache,
    /// Counters for the last rendered frame.
    pub stats: RenderStats,
    warned: bool,
}

impl Renderer {
    pub fn new(gpu: Arc<Gpu>) -> Renderer {
        let res = Resources::new(gpu);
        let device = &res.gpu.device;
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<GlobalsU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &res.pipes.globals_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&res.pipes.sampler) },
            ],
        });
        Renderer {
            res,
            msaa: None,
            uniforms: None,
            globals_buf,
            globals_bg,
            scene: SceneCache::new(),
            stats: RenderStats::default(),
            warned: false,
        }
    }

    fn msaa_view(&mut self, size: [u32; 2]) -> wgpu::TextureView {
        if let Some((v, s)) = &self.msaa {
            if *s == size {
                return v.clone();
            }
        }
        let tex = self.res.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
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

    /// A uniform ring large enough for `n` blocks.
    fn uniform_ring(&mut self, n: usize) -> wgpu::Buffer {
        let need = (n.max(1)) * UNIFORM_ALIGN;
        if let Some((b, cap)) = &self.uniforms {
            if *cap >= need {
                return b.clone();
            }
        }
        let cap = need.next_power_of_two().max(64 * 1024);
        let b = self.res.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: cap as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.uniforms = Some((b.clone(), cap));
        b
    }

    /// Encodes one frame of `dl` into `target` (size in device pixels).
    pub fn render(
        &mut self,
        dl: &DrawList,
        target: &wgpu::TextureView,
        size: [u32; 2],
        ppu: f64,
    ) -> wgpu::CommandBuffer {
        self.res.frame += 1;
        self.res.stats = RenderStats::default();
        let msaa = self.msaa_view(size);
        let g = GlobalsU { target_px: [size[0] as f32, size[1] as f32], ppu: ppu as f32, _pad: 0.0 };
        self.res.gpu.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&g));

        let ubuf = self.uniform_ring(dl.items.len());
        let mut uniforms: Vec<u8> = Vec::with_capacity(dl.items.len() * UNIFORM_ALIGN);
        let pipes = self.res.pipes.clone();
        let mut draws: Vec<([u32; 4], DrawCmd)> = Vec::with_capacity(dl.items.len());
        let full = [0, 0, size[0], size[1]];
        {
            let mut f = Frame { res: &mut self.res, pipes, uniforms: &mut uniforms, ubuf: ubuf.clone(), ppu, size };
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
                let cmd = match &item.prim {
                    Prim::Rects(r) => pipelines::mesh::prepare_rects(&mut f, r),
                    Prim::Mesh(m) => pipelines::mesh::prepare(&mut f, m, xform),
                    Prim::Markers(m) => pipelines::sprite::prepare(&mut f, m, xform),
                    Prim::Lines(_) | Prim::Glyphs(_) | Prim::Field(_) => {
                        if !self.warned {
                            log::debug!("ezviz: primitive not yet supported by the GPU backend");
                            self.warned = true;
                        }
                        None
                    }
                };
                if let Some(cmd) = cmd {
                    draws.push((scissor, cmd));
                }
            }
        }
        if !uniforms.is_empty() {
            self.res.gpu.queue.write_buffer(&ubuf, 0, &uniforms);
        }
        self.res.stats.draws = draws.len() as u32;

        let mut enc =
            self.res.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
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
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Discard },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bg, &[]);
            for (sc, d) in &draws {
                pass.set_scissor_rect(sc[0], sc[1], sc[2], sc[3]);
                pass.set_pipeline(&d.pipeline);
                pass.set_bind_group(1, &d.bind, &[d.offset]);
                if let Some(vb) = &d.vb {
                    pass.set_vertex_buffer(0, vb.slice(..));
                }
                pass.draw(d.vertices.clone(), d.instances.clone());
            }
        }
        self.res.evict();
        self.stats = self.res.stats;
        enc.finish()
    }

    /// Renders `dl` offscreen at `ppu` and reads back straight-alpha RGBA8 rows (top row first).
    pub fn render_rgba(&mut self, dl: &DrawList, ppu: f64) -> Result<(u32, u32, Vec<u8>)> {
        let w = (dl.size[0] * ppu).round().max(1.0) as u32;
        let h = (dl.size[1] * ppu).round().max(1.0) as u32;
        let max = self.res.gpu.max_texture_size();
        if w > max || h > max {
            return Err(Error::TooLarge(format!(
                "a {w}x{h} px image exceeds the GPU texture limit of {max} px; lower px_per_unit/dpi or the figure size"
            )));
        }
        let device = self.res.gpu.device.clone();
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
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
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let idx = self.res.gpu.queue.submit([frame_cmd, enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None })
            .map_err(|e| Error::Gpu(e.to_string()))?;
        rx.recv().map_err(|e| Error::Gpu(e.to_string()))?.map_err(|e| Error::Gpu(e.to_string()))?;
        let mut rgba = Vec::with_capacity((unpadded * h) as usize);
        {
            let data = slice.get_mapped_range().map_err(|e| Error::Gpu(e.to_string()))?;
            for row in data.chunks_exact(padded as usize) {
                for px in row[..unpadded as usize].chunks_exact(4) {
                    // BGRA premultiplied -> RGBA straight alpha.
                    let a = px[3];
                    let un = |v: u8| {
                        if a == 0 || a == 255 { v } else { ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 }
                    };
                    rgba.extend_from_slice(&[un(px[2]), un(px[1]), un(px[0]), a]);
                }
            }
        }
        buf.unmap();
        Ok((w, h, rgba))
    }
}

/// Clip rectangle (units) -> scissor (device px), clamped; `None` if empty.
fn scissor(r: Rect, ppu: f64, size: [u32; 2]) -> Option<[u32; 4]> {
    let x0 = (r.x * ppu).round().clamp(0.0, size[0] as f64) as u32;
    let y0 = (r.y * ppu).round().clamp(0.0, size[1] as f64) as u32;
    let x1 = (r.right() * ppu).round().clamp(0.0, size[0] as f64) as u32;
    let y1 = (r.bottom() * ppu).round().clamp(0.0, size[1] as f64) as u32;
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
}
