//! `lines3d`: 3D polylines of an Axis3 as instanced screen-space capsules, one instance per
//! segment. The point buffer is bound at two offsets (instance step, stride 12) so instance `k`
//! reads points `k` and `k + 1`; per-point colors (or values) likewise at two offsets.

use super::super::frame::{CMapU, DrawCmd, Frame, premul};
use super::Layouts;
use super::view3d::{View3U, points3, view_uniform};
use crate::scene::drawlist::{Lines3dPrim, PrimColor};
use bytemuck::{Pod, Zeroable};

pub(crate) const SHADER: &str = include_str!("lines3d.wgsl");

/// WGSL `Lines3dU`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Lines3dU {
    v: View3U,
    color: [f32; 4],
    width: f32,
    color_mode: u32,
    n: u32,
    _p: u32,
    cm: CMapU,
}

pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("lines3d"),
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
        "lines3d",
        shader,
        "vs_line3d",
        "fs_line3d",
        l,
        &l.lines3d,
        &[
            super::instance_attr(12, &wgpu::vertex_attr_array![0 => Float32x3]),
            super::instance_attr(12, &wgpu::vertex_attr_array![1 => Float32x3]),
            super::instance_attr(4, &wgpu::vertex_attr_array![2 => Uint32]),
            super::instance_attr(4, &wgpu::vertex_attr_array![3 => Uint32]),
        ],
        wgpu::PrimitiveTopology::TriangleStrip,
        format,
        false,
    )
}

pub(crate) fn prepare(f: &mut Frame, l: &Lines3dPrim) -> Option<DrawCmd> {
    let n = l.pts.len();
    let width = l.width as f64 * f.ppu;
    if n < 2 || n > u32::MAX as usize || !(width > 0.0) {
        return None;
    }
    let pts = points3(f, &l.pts, l.append);
    let (color_mode, color, col, cm, lut) = match &l.color {
        PrimColor::Uniform(c) => (0, premul(*c), None, CMapU::default(), f.dummy_lut()),
        PrimColor::PerElement(b) if b.len() >= n => (1, [0.0; 4], Some(f.vertex(b)), CMapU::default(), f.dummy_lut()),
        PrimColor::Values(b, map) if b.len() >= n => {
            (2, [0.0; 4], Some(f.vertex(b)), CMapU::from(map), f.lut(&map.lut))
        }
        _ => {
            crate::warn_once("3D line colors have fewer entries than points; drawing nothing");
            return None;
        }
    };
    let offset = f.push_uniform(&Lines3dU {
        v: view_uniform(&l.view, f.ppu, f.size),
        color,
        width: width as f32,
        color_mode,
        n: n as u32,
        _p: 0,
        cm,
    });
    let bind = f.device().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lines3d"),
        layout: &f.layouts().lines3d,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: f.uniform_binding::<Lines3dU>() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&lut) },
        ],
    });
    // Without per-point colors the color streams read the (large enough) point buffer, ignored.
    let (c1, c2) = match col {
        Some(b) => ((b.clone(), 0), (b, 4)),
        None => ((pts.clone(), 0), (pts.clone(), 0)),
    };
    Some(DrawCmd {
        pipeline: f.pipes.lines3d.clone(),
        bind,
        offset,
        vbs: vec![(pts.clone(), 0), (pts, 12), c1, c2],
        vertices: 0..4,
        instances: 0..(n - 1) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_layout_matches_wgsl() {
        // View3 (mat4 + 2 vec4) + color + 4 scalars + CMap (4 vec4); within one 256 B block.
        assert_eq!(std::mem::size_of::<Lines3dU>(), 96 + 16 + 16 + 64);
    }
}
