//! Shared pieces of the 3D pipelines (`lines3d`, `markers3d`, `mesh3d`): the camera uniform
//! (`View3` in `view3d.wgsl`), the depth-tested pipeline builder, and append-aware uploads of
//! 3D point buffers.
//!
//! 3D items are drawn in render passes of their own with a depth attachment ([`DEPTH_FORMAT`],
//! multisampled like the color target); see `Renderer::render_to`. Meshes write depth, lines and
//! markers only test against it.

use super::super::frame::{Cached, Frame, tag};
use super::Layouts;
use crate::data::points::split_append_rev;
use crate::scene::drawlist::{Buf, View3d};
use bytemuck::{Pod, Zeroable};

/// WGSL shared by the 3D pipelines (after `common.wgsl`).
pub(crate) const SHADER: &str = include_str!("view3d.wgsl");

/// Depth buffer format of 3D passes (renderable and multisampled on WebGPU and WebGL2).
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;

/// WGSL `View3`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct View3U {
    /// Local -> target clip space (column-major), depth 0..1.
    m: [[f32; 4]; 4],
    /// Clip box in local coordinates; `clip_lo.w = 1` enables it.
    clip_lo: [f32; 4],
    clip_hi: [f32; 4],
}

/// The camera of `v` for a `size` px target at `ppu` px per unit.
pub(crate) fn view_uniform(v: &View3d, ppu: f64, size: [u32; 2]) -> View3U {
    let (w, h) = (size[0].max(1) as f64, size[1].max(1) as f64);
    let a = v.area;
    // Makie's clip space of the scene area (z in -1..1) -> clip space of the target (z in 0..1).
    let ax = a.w * ppu / w;
    let bx = 2.0 * a.x * ppu / w + ax - 1.0;
    let ay = a.h * ppu / h;
    let by = 1.0 - 2.0 * a.y * ppu / h - ay;
    let t = [[ax, 0.0, 0.0, bx], [0.0, ay, 0.0, by], [0.0, 0.0, 0.5, 0.5], [0.0, 0.0, 0.0, 1.0]];
    let m = crate::scene::axis3::camera::mul(&t, &v.mvp);
    let cols: [[f32; 4]; 4] = std::array::from_fn(|c| std::array::from_fn(|r| m[r][c] as f32));
    let (clip_lo, clip_hi) = match v.clip {
        Some([lo, hi]) => ([lo[0], lo[1], lo[2], 1.0], [hi[0], hi[1], hi[2], 0.0]),
        None => ([0.0; 4], [0.0; 4]),
    };
    View3U { m: cols, clip_lo, clip_hi }
}

/// A pipeline like `super::pipeline`, with a depth test (and depth writes if `write`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn pipeline(
    device: &wgpu::Device,
    label: &str,
    shader: &wgpu::ShaderModule,
    vs: &str,
    fs: &str,
    l: &Layouts,
    layout: &wgpu::BindGroupLayout,
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    topology: wgpu::PrimitiveTopology,
    format: wgpu::TextureFormat,
    write: bool,
) -> wgpu::RenderPipeline {
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(&l.globals), Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vs),
            compilation_options: Default::default(),
            buffers,
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &super::target(format),
        }),
        primitive: wgpu::PrimitiveState { topology, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(write),
            depth_compare: Some(if write { wgpu::CompareFunction::Less } else { wgpu::CompareFunction::LessEqual }),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: super::multisample(),
        multiview_mask: None,
        cache: None,
    })
}

/// A vertex buffer holding the 3D points of `b`. Append-only buffers (`append`) whose GPU copy
/// is an older revision of the same generation only upload the new tail.
pub(crate) fn points3(f: &mut Frame, b: &Buf<[f32; 3]>, append: bool) -> wgpu::Buffer {
    let bytes: &[u8] = bytemuck::cast_slice(b.data.as_slice());
    let Some(k) = b.key else { return f.transient(bytes, wgpu::BufferUsages::VERTEX) };
    if !append {
        return f.vertex(b);
    }
    let key = (k.uid, k.part, tag::RAW);
    let frame = f.res.frame;
    let n = b.data.len();
    if let Some(c) = f.res.cache.get_mut(&key) {
        if c.rev == k.rev {
            c.frame = frame;
            return c.buf.clone();
        }
        if c.buf.size() >= bytes.len() as u64 {
            let (g0, n0) = split_append_rev(c.rev);
            let (g1, n1) = split_append_rev(k.rev);
            let start = if g0 == g1 && n0 < n1 && n1 == n { n0 } else { 0 };
            f.res.gpu.queue.write_buffer(&c.buf, start as u64 * 12, &bytes[start * 12..]);
            f.res.stats.data_bytes += (bytes.len() - start * 12) as u64;
            c.rev = k.rev;
            c.frame = frame;
            return c.buf.clone();
        }
    }
    let size = (bytes.len() as u64 * 3 / 2).max(16).next_multiple_of(4);
    let buf = f.res.gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("points3"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    f.res.gpu.queue.write_buffer(&buf, 0, bytes);
    f.res.stats.data_bytes += bytes.len() as u64;
    f.res.cache.insert(key, Cached { rev: k.rev, buf: buf.clone(), frame, pads: None, dups: None });
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_uniform_maps_the_area() {
        use crate::scene::drawlist::Rect;
        let v = View3d {
            mvp: crate::scene::axis3::camera::identity(),
            area: Rect::new(10.0, 20.0, 100.0, 50.0),
            world_scale: [1.0; 3],
            world_offset: [0.0; 3],
            normal_scale: [1.0; 3],
            clip: None,
            light_dir: [0.0, 0.0, -1.0],
            eye: [0.0, 0.0, 10.0],
            ambient: 0.45,
            light_color: 0.5,
            group: 1,
        };
        let u = view_uniform(&v, 2.0, [400, 200]);
        let m = |c: usize, r: usize| u.m[c][r] as f64;
        // Area clip (-1, 1) = top-left -> px (20, 40) at ppu 2 -> target clip.
        let (x, y) = (m(0, 0) * -1.0 + m(3, 0), m(1, 1) * 1.0 + m(3, 1));
        assert!((x - (2.0 * 20.0 / 400.0 - 1.0)).abs() < 1e-6);
        assert!((y - (1.0 - 2.0 * 40.0 / 200.0)).abs() < 1e-6);
        // Depth -1..1 -> 0..1.
        assert!((m(2, 2) - 0.5).abs() < 1e-9 && (m(3, 2) - 0.5).abs() < 1e-9);
    }
}
