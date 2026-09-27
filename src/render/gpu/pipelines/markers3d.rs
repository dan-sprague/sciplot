//! `markers3d`: Axis3 scatter markers as instanced screen-space quads with analytic SDFs (the
//! shapes of `sprite`), at the depth of their 3D centre. Per-marker positions (slot 0, `vec3`),
//! colors or colormap values (slot 1) and sizes (slot 2) come from instance-step vertex buffers.

use super::super::frame::{CMapU, DrawCmd, Frame, premul};
use super::Layouts;
use super::view3d::{View3U, points3, view_uniform};
use crate::scene::drawlist::{Markers3dPrim, PrimColor};
use bytemuck::{Pod, Zeroable};

pub(crate) const SHADER: &str = include_str!("markers3d.wgsl");

/// WGSL `Markers3dU`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Markers3dU {
    v: View3U,
    color: [f32; 4],
    stroke_color: [f32; 4],
    size: f32,
    stroke: f32,
    shape: u32,
    col_mode: u32,
    cm: CMapU,
}

pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("markers3d"),
        entries: &[super::uniform_entry(0, true), super::texture_entry(1, true)],
    })
}

pub(crate) fn pipeline(
    device: &wgpu::Device,
    l: &Layouts,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    super::view3d::pipeline(
        device,
        "markers3d",
        shader,
        "vs_marker3d",
        "fs_marker3d",
        l,
        &l.markers3d,
        &[
            super::instance_attr(12, &wgpu::vertex_attr_array![0 => Float32x3]),
            super::instance_attr(4, &wgpu::vertex_attr_array![1 => Uint32]),
            super::instance_attr(4, &wgpu::vertex_attr_array![2 => Float32]),
        ],
        wgpu::PrimitiveTopology::TriangleStrip,
        format,
        false,
    )
}

pub(crate) fn prepare(f: &mut Frame, m: &Markers3dPrim) -> Option<DrawCmd> {
    let mut n = m.pos.len();
    if n == 0 || n > u32::MAX as usize {
        return None;
    }
    let pos = points3(f, &m.pos, m.append);
    let (mut col_mode, color, col, cm, lut) = match &m.color {
        PrimColor::Uniform(c) => (0, premul(*c), None, CMapU::default(), f.dummy_lut()),
        PrimColor::PerElement(b) => {
            n = n.min(b.len());
            (1, [0.0; 4], Some(f.vertex(b)), CMapU::default(), f.dummy_lut())
        }
        PrimColor::Values(b, map) => {
            n = n.min(b.len());
            (2, [0.0; 4], Some(f.vertex(b)), CMapU::from(map), f.lut(&map.lut))
        }
    };
    let sizes = match &m.sizes {
        Some(s) => {
            n = n.min(s.len());
            col_mode |= 16;
            Some(f.vertex(s))
        }
        None => None,
    };
    if n == 0 {
        return None;
    }
    let sw = if m.stroke_color.a > 0.0 { m.stroke_width.max(0.0) } else { 0.0 };
    let offset = f.push_uniform(&Markers3dU {
        v: view_uniform(&m.view, f.ppu, f.size),
        color,
        stroke_color: premul(m.stroke_color),
        size: (m.size as f64 * f.ppu) as f32,
        stroke: (sw as f64 * f.ppu) as f32,
        shape: m.marker.shader_id(),
        col_mode,
        cm,
    });
    let bind = f.device().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("markers3d"),
        layout: &f.layouts().markers3d,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: f.uniform_binding::<Markers3dU>() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&lut) },
        ],
    });
    let col = col.unwrap_or_else(|| pos.clone());
    let sizes = sizes.unwrap_or_else(|| pos.clone());
    Some(DrawCmd {
        pipeline: f.pipes.markers3d.clone(),
        bind,
        offset,
        vbs: vec![(pos, 0), (col, 0), (sizes, 0)],
        vertices: 0..4,
        instances: 0..n as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_layout_matches_wgsl() {
        assert_eq!(std::mem::size_of::<Markers3dU>(), 96 + 16 + 16 + 16 + 64);
    }
}
