//! `sprite`: scatter markers as instanced quads with analytic SDFs. Per-marker data comes from
//! instance-step vertex buffers: positions (slot 0), colors or colormap values (slot 1, `u32`:
//! premultiplied RGBA8 or f32 bits) and sizes (slot 2). Streams a primitive doesn't have are
//! bound to the position buffer and ignored by the shader (uniform values come from `SpriteU`).

use super::super::frame::{CMapU, DrawCmd, Frame, POINTS_OFFSET, premul};
use super::Layouts;
use crate::scene::drawlist::{MarkersPrim, PrimColor};
use bytemuck::{Pod, Zeroable};

pub(crate) const SHADER: &str = include_str!("sprite.wgsl");

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

pub(crate) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("sprite"),
        entries: &[super::uniform_entry(0, true), super::texture_entry(1, true)],
    })
}

pub(crate) fn pipeline(
    device: &wgpu::Device,
    l: &Layouts,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    super::pipeline(
        device,
        "sprite",
        shader,
        "vs_marker",
        "fs_marker",
        l,
        &l.sprite,
        &[
            super::instance_attr(8, &wgpu::vertex_attr_array![0 => Float32x2]),
            super::instance_attr(4, &wgpu::vertex_attr_array![1 => Uint32]),
            super::instance_attr(4, &wgpu::vertex_attr_array![2 => Float32]),
        ],
        wgpu::PrimitiveTopology::TriangleStrip,
        format,
    )
}

pub(crate) fn prepare(f: &mut Frame, m: &MarkersPrim, xform: [f32; 4]) -> Option<DrawCmd> {
    let mut n = m.pos.data.len();
    if n == 0 || n > u32::MAX as usize {
        return None;
    }
    let pos = f.points(&m.pos, false, false);
    let (col_mode, color, col, cm, lut) = match &m.color {
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
    let (size_stride, sz) = match &m.sizes {
        Some(s) => {
            n = n.min(s.len());
            (1, Some(f.vertex(s)))
        }
        None => (0, None),
    };
    if n < m.pos.data.len() {
        crate::warn_once("marker colors or sizes have fewer entries than points; extra points are not drawn");
    }
    if n == 0 {
        return None;
    }
    let offset = f.push_uniform(&SpriteU {
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
    });
    let bind = f.device().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sprite"),
        layout: &f.layouts().sprite,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: f.uniform_binding::<SpriteU>() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&lut) },
        ],
    });
    // Absent streams read the (large enough) position buffer; the shader ignores them.
    let col = col.map_or((pos.clone(), POINTS_OFFSET), |b| (b, 0));
    let sz = sz.map_or((pos.clone(), POINTS_OFFSET), |b| (b, 0));
    Some(DrawCmd {
        pipeline: f.pipes.sprite.clone(),
        bind,
        offset,
        vbs: vec![(pos, POINTS_OFFSET), col, sz],
        vertices: 0..4,
        instances: 0..n as u32,
    })
}
