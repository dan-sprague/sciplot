//! Per-context renderer: owns GPU caches for one window or one export and turns a `DrawList`
//! into a single 4x MSAA render pass (painter's order, scissor per item).

use super::frame::{DrawCmd, Frame, RenderStats, Resources, UNIFORM_ALIGN};
use super::pipelines;
use super::{Gpu, MSAA, OFFSCREEN_FORMAT, TARGET_FORMAT};
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
    msaa: Option<(wgpu::TextureView, [u32; 2], wgpu::TextureFormat)>,
    /// Multisampled depth buffer of 3D passes (created when a frame has 3D items).
    depth: Option<(wgpu::TextureView, [u32; 2])>,
    uniforms: Option<(wgpu::Buffer, usize)>,
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    /// Scene-building memo for this context.
    pub scene: SceneCache,
    /// Counters for the last rendered frame.
    pub stats: RenderStats,
    /// Uniform block alignment (the WebGPU 256 B, or more if the device needs it).
    ualign: usize,
    /// Format of offscreen targets (`render_rgba`).
    offscreen_format: wgpu::TextureFormat,
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
            layout: &res.layouts.globals,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&res.layouts.sampler) },
            ],
        });
        let ualign = UNIFORM_ALIGN.max(device.limits().min_uniform_buffer_offset_alignment as usize);
        Renderer {
            res,
            msaa: None,
            depth: None,
            uniforms: None,
            globals_buf,
            globals_bg,
            scene: SceneCache::new(),
            stats: RenderStats::default(),
            ualign,
            offscreen_format: OFFSCREEN_FORMAT,
        }
    }

    /// Renders offscreen images ([`Renderer::render_rgba`]) into `format` targets
    /// (`Rgba8Unorm` or `Bgra8Unorm`).
    pub fn set_offscreen_format(&mut self, format: wgpu::TextureFormat) {
        self.offscreen_format = format;
    }

    fn msaa_view(&mut self, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::TextureView {
        if let Some((v, s, f)) = &self.msaa
            && *s == size
            && *f == format
        {
            return v.clone();
        }
        let tex = self.res.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: MSAA,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let v = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.msaa = Some((v.clone(), size, format));
        v
    }

    fn depth_view(&mut self, size: [u32; 2]) -> wgpu::TextureView {
        if let Some((v, s)) = &self.depth
            && *s == size
        {
            return v.clone();
        }
        let tex = self.res.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: MSAA,
            dimension: wgpu::TextureDimension::D2,
            format: pipelines::view3d::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let v = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth = Some((v.clone(), size));
        v
    }

    /// A uniform ring large enough for `n` blocks.
    fn uniform_ring(&mut self, n: usize) -> wgpu::Buffer {
        let need = n.max(1) * self.ualign;
        if let Some((b, cap)) = &self.uniforms
            && *cap >= need
        {
            return b.clone();
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

    /// Encodes one frame of `dl` into a window surface `target` of [`TARGET_FORMAT`] (size in
    /// device pixels).
    pub fn render(
        &mut self,
        dl: &DrawList,
        target: &wgpu::TextureView,
        size: [u32; 2],
        ppu: f64,
    ) -> wgpu::CommandBuffer {
        self.render_to(dl, target, TARGET_FORMAT, size, ppu)
    }

    /// Encodes one frame of `dl` into `target` of `format` (size in device pixels).
    pub fn render_to(
        &mut self,
        dl: &DrawList,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        ppu: f64,
    ) -> wgpu::CommandBuffer {
        self.res.frame += 1;
        self.res.stats = RenderStats::default();
        let msaa = self.msaa_view(size, format);
        let g = GlobalsU { target_px: [size[0] as f32, size[1] as f32], ppu: ppu as f32, _pad: 0.0 };
        self.res.gpu.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&g));

        let ubuf = self.uniform_ring(dl.items.len());
        let mut uniforms: Vec<u8> = Vec::with_capacity(dl.items.len() * self.ualign);
        let pipes = self.res.gpu.pipelines(format);
        let mut draws: Vec<([u32; 4], DrawCmd)> = Vec::with_capacity(dl.items.len());
        // The depth group of each draw (3D items of one Axis3; `None` for 2D items).
        let mut groups: Vec<Option<u64>> = Vec::with_capacity(dl.items.len());
        let full = [0, 0, size[0], size[1]];
        {
            let mut f = Frame {
                res: &mut self.res,
                pipes,
                uniforms: &mut uniforms,
                ubuf: ubuf.clone(),
                ualign: self.ualign,
                ppu,
                size,
            };
            pipelines::glyph::begin_frame(&mut f, dl);
            for item in &dl.items {
                let scissor = match item.clip {
                    Some(r) => match scissor(r, ppu, size) {
                        Some(s) => s,
                        None => continue,
                    },
                    None => full,
                };
                let aff = match item.space {
                    Space::Figure => [ppu, ppu, 0.0, 0.0],
                    Space::Data(i) => match dl.axes.get(i as usize) {
                        Some(a) => a.affine(ppu),
                        None => continue,
                    },
                };
                let xform: [f32; 4] = aff.map(|v| v as f32);
                let cmd = match &item.prim {
                    Prim::Rects(r) => pipelines::mesh::prepare_rects(&mut f, r),
                    Prim::Mesh(m) => pipelines::mesh::prepare(&mut f, m, xform),
                    Prim::Markers(m) => pipelines::sprite::prepare(&mut f, m, xform),
                    Prim::Lines(l) => pipelines::line::prepare(&mut f, l, xform),
                    Prim::Glyphs(g) => pipelines::glyph::prepare(&mut f, g, xform),
                    Prim::Field(p) => {
                        let mut cmds = pipelines::field::prepare(&mut f, p, aff);
                        let last = cmds.pop();
                        groups.extend(cmds.iter().map(|_| None));
                        draws.extend(cmds.into_iter().map(|c| (scissor, c)));
                        last
                    }
                    Prim::Lines3d(l) => pipelines::lines3d::prepare(&mut f, l),
                    Prim::Markers3d(m) => pipelines::markers3d::prepare(&mut f, m),
                    Prim::Mesh3d(m) => pipelines::mesh3d::prepare(&mut f, m),
                };
                if let Some(cmd) = cmd {
                    draws.push((scissor, cmd));
                    groups.push(match &item.prim {
                        Prim::Lines3d(l) => Some(l.view.group),
                        Prim::Markers3d(m) => Some(m.view.group),
                        Prim::Mesh3d(m) => Some(m.view.group),
                        _ => None,
                    });
                }
            }
        }
        if !uniforms.is_empty() {
            self.res.gpu.queue.write_buffer(&ubuf, 0, &uniforms);
        }
        self.res.stats.draws = draws.len() as u32;

        let mut enc =
            self.res.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        // One render pass per run of draws with the same depth group: 2D runs have no depth
        // attachment (a frame without 3D items is a single pass, as before); 3D runs share a
        // multisampled depth buffer, cleared when a new group starts.
        let mut runs: Vec<(usize, usize, Option<u64>)> = Vec::new();
        for (i, g) in groups.iter().enumerate() {
            match runs.last_mut() {
                Some(r) if r.2 == *g => r.1 = i + 1,
                _ => runs.push((i, i + 1, *g)),
            }
        }
        if runs.is_empty() {
            runs.push((0, 0, None));
        }
        let depth = runs.iter().any(|r| r.2.is_some()).then(|| self.depth_view(size));
        let bg = dl.background;
        let clear = wgpu::Color {
            r: (bg.r * bg.a) as f64,
            g: (bg.g * bg.a) as f64,
            b: (bg.b * bg.a) as f64,
            a: bg.a as f64,
        };
        let mut cleared_group = None;
        for (k, &(start, end, group)) in runs.iter().enumerate() {
            let last = k + 1 == runs.len();
            let depth_attachment = match (group, &depth) {
                (Some(gid), Some(view)) => {
                    let load = if cleared_group == Some(gid) { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(1.0) };
                    cleared_group = Some(gid);
                    Some(wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations { load, store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    })
                }
                _ => None,
            };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("figure"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &msaa,
                    depth_slice: None,
                    resolve_target: last.then_some(target),
                    ops: wgpu::Operations {
                        load: if k == 0 { wgpu::LoadOp::Clear(clear) } else { wgpu::LoadOp::Load },
                        store: if last { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                    },
                })],
                depth_stencil_attachment: depth_attachment,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_bg, &[]);
            for (sc, d) in &draws[start..end] {
                pass.set_scissor_rect(sc[0], sc[1], sc[2], sc[3]);
                pass.set_pipeline(&d.pipeline);
                pass.set_bind_group(1, &d.bind, &[d.offset]);
                for (slot, (vb, off)) in d.vbs.iter().enumerate() {
                    pass.set_vertex_buffer(slot as u32, vb.slice(*off..));
                }
                pass.draw(d.vertices.clone(), d.instances.clone());
            }
        }
        self.res.evict();
        self.stats = self.res.stats;
        enc.finish()
    }

    /// Renders `dl` offscreen at `ppu` and reads back straight-alpha RGBA8 rows (top row first).
    /// Blocks until the GPU is done (native only: the browser can't wait for a readback).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_rgba(&mut self, dl: &DrawList, ppu: f64) -> Result<(u32, u32, Vec<u8>)> {
        let w = (dl.size[0] * ppu).round().max(1.0) as u32;
        let h = (dl.size[1] * ppu).round().max(1.0) as u32;
        let max = self.res.gpu.max_texture_size();
        if w > max || h > max {
            return Err(Error::TooLarge(format!(
                "a {w}x{h} px image exceeds the GPU texture limit of {max} px; lower px_per_unit/dpi or the figure size"
            )));
        }
        let format = self.offscreen_format;
        let bgra = match format {
            wgpu::TextureFormat::Rgba8Unorm => false,
            wgpu::TextureFormat::Bgra8Unorm => true,
            f => return Err(Error::Gpu(format!("unsupported offscreen format {f:?}"))),
        };
        let device = self.res.gpu.device.clone();
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let frame_cmd = self.render_to(dl, &view, format, [w, h], ppu);

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
        let rgba = {
            let data = slice.get_mapped_range().map_err(|e| Error::Gpu(e.to_string()))?;
            unpremultiply_rows(&data, padded as usize, unpadded as usize, bgra)
        };
        buf.unmap();
        Ok((w, h, rgba))
    }
}

#[cfg(target_arch = "wasm32")]
impl Renderer {
    /// Offscreen readback needs to block on the GPU, which the browser can't do.
    pub fn render_rgba(&mut self, _dl: &DrawList, _ppu: f64) -> Result<(u32, u32, Vec<u8>)> {
        Err(Error::Gpu("synchronous GPU readback is not available in the browser".into()))
    }
}

/// Premultiplied RGBA or BGRA rows (`stride` bytes apart, `width` bytes used) -> straight-alpha
/// RGBA.
fn unpremultiply_rows(data: &[u8], stride: usize, width: usize, bgra: bool) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(width * (data.len() / stride.max(1)));
    for row in data.chunks_exact(stride) {
        for px in row[..width].as_chunks::<4>().0 {
            let a = px[3];
            let un = |v: u8| {
                if a == 0 || a == 255 { v } else { ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 }
            };
            let (r, b) = if bgra { (px[2], px[0]) } else { (px[0], px[2]) };
            rgba.extend_from_slice(&[un(r), un(px[1]), un(b), a]);
        }
    }
    rgba
}

/// Clip rectangle (units) -> scissor (device px), clamped; `None` if empty.
fn scissor(r: Rect, ppu: f64, size: [u32; 2]) -> Option<[u32; 4]> {
    let x0 = (r.x * ppu).round().clamp(0.0, size[0] as f64) as u32;
    let y0 = (r.y * ppu).round().clamp(0.0, size[1] as f64) as u32;
    let x1 = (r.right() * ppu).round().clamp(0.0, size[0] as f64) as u32;
    let y1 = (r.bottom() * ppu).round().clamp(0.0, size[1] as f64) as u32;
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readback_handles_both_channel_orders() {
        let rgba = [200u8, 100, 50, 255, 64, 32, 16, 128, 0, 0, 0, 0, 0, 0];
        let bgra = [50u8, 100, 200, 255, 16, 32, 64, 128, 0, 0, 0, 0, 0, 0];
        let a = unpremultiply_rows(&rgba, 14, 12, false);
        let b = unpremultiply_rows(&bgra, 14, 12, true);
        assert_eq!(a, b);
        assert_eq!(&a[..8], &[200, 100, 50, 255, 128, 64, 32, 128]);
    }
}
