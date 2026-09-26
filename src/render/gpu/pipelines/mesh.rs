//! `mesh`: filled triangles (fills, legend patches) and pixel-snapped decoration rectangles.

use super::super::frame::{DrawCmd, Frame};
use super::Layouts;
use crate::scene::drawlist::{MeshPrim, MeshVertex, RectPrim};
use bytemuck::{Pod, Zeroable};

pub(crate) const SHADER: &str = include_str!("mesh.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MeshU {
    xform: [f32; 4],
}

pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mesh"),
        entries: &[super::uniform_entry(0, true)],
    })
}

pub(crate) fn pipeline(
    device: &wgpu::Device,
    l: &Layouts,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let vb = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<MeshVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Unorm8x4],
    };
    super::pipeline(
        device,
        "mesh",
        shader,
        "vs_mesh",
        "fs_mesh",
        l,
        &l.mesh,
        &[Some(vb)],
        wgpu::PrimitiveTopology::TriangleList,
        format,
    )
}

fn draw(f: &mut Frame, vb: wgpu::Buffer, count: u32, xform: [f32; 4]) -> DrawCmd {
    let offset = f.push_uniform(&MeshU { xform });
    let bind = f.device().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mesh"),
        layout: &f.layouts().mesh,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: f.uniform_binding::<MeshU>() }],
    });
    DrawCmd { pipeline: f.pipes.mesh.clone(), bind, offset, vbs: vec![(vb, 0)], vertices: 0..count, instances: 0..1 }
}

pub(crate) fn prepare(f: &mut Frame, m: &MeshPrim, xform: [f32; 4]) -> Option<DrawCmd> {
    if m.verts.data.is_empty() {
        return None;
    }
    let vb = f.vertex(&m.verts);
    Some(draw(f, vb, m.verts.data.len() as u32, xform))
}

pub(crate) fn prepare_rects(f: &mut Frame, rects: &[RectPrim]) -> Option<DrawCmd> {
    let verts = rect_vertices(rects, f.ppu);
    if verts.is_empty() {
        return None;
    }
    let vb = f.transient(bytemuck::cast_slice(&verts), wgpu::BufferUsages::VERTEX);
    Some(draw(f, vb, verts.len() as u32, [1.0, 1.0, 0.0, 0.0]))
}

/// Snaps the center of a thin line so it covers whole device pixels: odd widths center on a pixel
/// center, even widths on a pixel edge. The width itself is never changed.
fn snap_center(c: f64, width_px: f64) -> f64 {
    if (width_px.round() as i64) % 2 == 1 { c.floor() + 0.5 } else { c.round() }
}

/// Rect primitives -> triangle vertices in device pixels.
pub(crate) fn rect_vertices(rects: &[RectPrim], ppu: f64) -> Vec<MeshVertex> {
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
        let p = |x: f64, y: f64| MeshVertex { pos: [x as f32, y as f32], color: c };
        v.extend_from_slice(&[p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y0), p(x1, y1), p(x0, y1)]);
    }
    v
}
